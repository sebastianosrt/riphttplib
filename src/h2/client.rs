use crate::h2::connection::H2Connection;
use crate::types::{FrameType, FrameTypeH2, Header, Protocol, ProtocolError, Response, Target};
use async_trait::async_trait;
use bytes::Bytes;

pub struct H2Client;

// HTTP/2 Error Codes for RST_STREAM frames (RFC 7540 Section 7)
pub const NO_ERROR: u32 = 0x0;
pub const PROTOCOL_ERROR: u32 = 0x1;
pub const INTERNAL_ERROR: u32 = 0x2;
pub const FLOW_CONTROL_ERROR: u32 = 0x3;
pub const SETTINGS_TIMEOUT: u32 = 0x4;
pub const STREAM_CLOSED: u32 = 0x5;
pub const FRAME_SIZE_ERROR: u32 = 0x6;
pub const REFUSED_STREAM: u32 = 0x7;
pub const CANCEL: u32 = 0x8;
pub const COMPRESSION_ERROR: u32 = 0x9;
pub const CONNECT_ERROR: u32 = 0xa;
pub const ENHANCE_YOUR_CALM: u32 = 0xb;
pub const INADEQUATE_SECURITY: u32 = 0xc;
pub const HTTP_1_1_REQUIRED: u32 = 0xd;

impl H2Client {
    pub fn new() -> Self {
        Self
    }

    async fn send_request(
        &self,
        connection: &mut H2Connection,
        method: &str,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<&Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<u32, ProtocolError> {
        // Create stream
        let stream_id = connection.create_stream().await?;

        // Separate pseudo-headers from regular headers
        let mut pseudo_headers = vec![];
        let mut regular_headers = vec![];

        if let Some(headers) = &headers {
            for header in headers {
                // Normalize header names to lowercase for HTTP/2 compliance
                let normalized_header = Header {
                    name: header.name.to_lowercase(),
                    value: header.value.clone(),
                };

                if normalized_header.name.starts_with(':') {
                    pseudo_headers.push(normalized_header);
                } else {
                    regular_headers.push(normalized_header);
                }
            }
        }

        // Method-specific validation and pseudo-header handling
        let method_upper = method.to_uppercase();

        // Add required pseudo-headers based on method
        if !pseudo_headers.iter().any(|h| h.name == ":method") {
            pseudo_headers.insert(0, Header::new(":method".to_string(), method.to_string()));
        }

        match method_upper.as_str() {
            "CONNECT" => {
                // CONNECT method: no :scheme or :path, :authority is required
                if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                    pseudo_headers.push(Header::new(
                        ":authority".to_string(),
                        format!("{}:{}", target.host, target.port),
                    ));
                }
                // Remove :scheme and :path if present
                pseudo_headers.retain(|h| h.name != ":scheme" && h.name != ":path");
            }
            "OPTIONS" => {
                // OPTIONS * method
                if target.path == "*" {
                    if !pseudo_headers.iter().any(|h| h.name == ":path") {
                        pseudo_headers.push(Header::new(":path".to_string(), "*".to_string()));
                    }
                } else {
                    if !pseudo_headers.iter().any(|h| h.name == ":path") {
                        pseudo_headers.push(Header::new(":path".to_string(), target.path.clone()));
                    }
                    if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                        pseudo_headers.push(Header::new(
                            ":authority".to_string(),
                            format!("{}:{}", target.host, target.port),
                        ));
                    }
                }
                if !pseudo_headers.iter().any(|h| h.name == ":scheme") {
                    pseudo_headers.push(Header::new(":scheme".to_string(), target.scheme.clone()));
                }
            }
            _ => {
                // Standard methods: require all pseudo-headers
                if !pseudo_headers.iter().any(|h| h.name == ":path") {
                    pseudo_headers.push(Header::new(":path".to_string(), target.path.clone()));
                }
                if !pseudo_headers.iter().any(|h| h.name == ":scheme") {
                    pseudo_headers.push(Header::new(":scheme".to_string(), target.scheme.clone()));
                }
                if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                    pseudo_headers.push(Header::new(
                        ":authority".to_string(),
                        format!("{}:{}", target.host, target.port),
                    ));
                }
            }
        }

        // Build final header list: pseudo-headers first, then regular headers
        let mut h2_headers = pseudo_headers;
        h2_headers.extend(regular_headers);

        let has_body = body.is_some() && !body.unwrap().is_empty();
        let has_trailers = trailers.is_some() && !trailers.as_ref().unwrap().is_empty();

        // Validate headers before sending
        for header in &h2_headers {
            if header.name.is_empty() {
                return Err(ProtocolError::MalformedHeaders(
                    "Empty header name".to_string(),
                ));
            }
            if header.name.contains(char::is_whitespace) {
                return Err(ProtocolError::MalformedHeaders(format!(
                    "Header name contains whitespace: {}",
                    header.name
                )));
            }
        }

        // Send HEADERS frame
        let end_stream = !has_body && !has_trailers;
        connection
            .send_headers(stream_id, &h2_headers, end_stream)
            .await
            .map_err(|e| ProtocolError::H2StreamError(format!("Failed to send headers: {}", e)))?;

        // Send DATA frame if body exists
        if let Some(body) = body {
            if !body.is_empty() {
                let end_stream = !has_trailers;
                connection
                    .send_data(stream_id, body, end_stream)
                    .await
                    .map_err(|e| {
                        ProtocolError::H2StreamError(format!("Failed to send data: {}", e))
                    })?;
            }
        }

        // Send trailer HEADERS frame if trailers exist
        if let Some(trailers) = trailers {
            if !trailers.is_empty() {
                connection
                    .send_headers(stream_id, &trailers, true)
                    .await
                    .map_err(|e| {
                        ProtocolError::H2StreamError(format!("Failed to send trailers: {}", e))
                    })?;
            }
        }

        Ok(stream_id)
    }

    async fn get_response(
        &self,
        connection: &mut H2Connection,
        _stream_id: u32,
    ) -> Result<Response, ProtocolError> {
        let mut status = 0u16;
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut trailers = None;
        let protocol_version = "HTTP/2.0".to_string();

        loop {
            let frame = connection.read_frame().await?;

            match &frame.frame_type {
                FrameType::H2(FrameTypeH2::Headers) => {
                    let decoded_headers = frame.decode_headers()?;

                    // Look for :status pseudo-header
                    for header in &decoded_headers {
                        if header.name == ":status" {
                            if let Some(ref status_str) = header.value {
                                status = status_str.parse::<u16>().unwrap_or(500);
                            }
                        } else if !header.name.starts_with(':') {
                            headers.push(header.clone());
                        }
                    }

                    connection.handle_frame(&frame).await?;

                    if frame.is_end_stream() {
                        break;
                    }
                }
                FrameType::H2(FrameTypeH2::Data) => {
                    body.extend_from_slice(&frame.payload);
                    connection.handle_frame(&frame).await?;

                    if frame.is_end_stream() {
                        break;
                    }
                }
                FrameType::H2(FrameTypeH2::Continuation) => {
                    let decoded_headers = frame.decode_headers()?;

                    for header in &decoded_headers {
                        if !header.name.starts_with(':') {
                            if trailers.is_none() {
                                trailers = Some(Vec::new());
                            }
                            trailers.as_mut().unwrap().push(header.clone());
                        }
                    }

                    connection.handle_frame(&frame).await?;

                    if frame.is_end_stream() {
                        break;
                    }
                }
                FrameType::H2(FrameTypeH2::RstStream) => {
                    connection.handle_frame(&frame).await?;
                    return Err(ProtocolError::RequestFailed("Stream was reset".to_string()));
                }
                _ => {
                    connection.handle_frame(&frame).await?;
                }
            }
        }

        Ok(Response {
            status,
            protocol_version,
            headers,
            body: Bytes::from(body),
            trailers,
        })
    }

    pub async fn send_raw_frame(
        &self,
        connection: &mut H2Connection,
        stream_id: u32,
        error_code: u32,
    ) -> Result<(), ProtocolError> {
        connection.send_rst(stream_id, error_code).await
    }

    pub async fn rst(
        &self,
        connection: &mut H2Connection,
        stream_id: u32,
    ) -> Result<(), ProtocolError> {
        connection.send_rst(stream_id, CANCEL).await
    }
}

#[async_trait]
impl Protocol for H2Client {
    async fn send(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(Bytes::from);
        self.send_bytes(target, method, headers, body_bytes, trailers)
            .await
    }

    async fn get(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        self.send(target, Some("GET".to_string()), headers, body, trailers)
            .await
    }

    async fn post(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        self.send(target, Some("POST".to_string()), headers, body, trailers)
            .await
    }

    async fn send_bytes(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let method = method.unwrap_or_else(|| "GET".to_string());

        // Create and establish HTTP/2 connection
        let mut connection = H2Connection::connect(target).await?;

        // Send request and get response
        let stream_id = self
            .send_request(
                &mut connection,
                &method,
                target,
                headers,
                body.as_ref(),
                trailers,
            )
            .await?;
        self.get_response(&mut connection, stream_id).await
    }

    async fn get_bytes(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        self.send_bytes(target, Some("GET".to_string()), headers, body, trailers)
            .await
    }

    async fn post_bytes(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        self.send_bytes(target, Some("POST".to_string()), headers, body, trailers)
            .await
    }
}
