use crate::h3::connection::H3Connection;
use crate::types::{FrameType, FrameTypeH3, Header, Protocol, ProtocolError, Response, Target};
use async_trait::async_trait;
use bytes::Bytes;

pub struct H3Client;

impl H3Client {
    pub fn new() -> Self {
        Self
    }

    async fn send_request(
        &self,
        connection: &mut H3Connection,
        method: &str,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<&Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<u32, ProtocolError> {
        // Create a new bidirectional stream for this request
        let (stream_id, mut send_stream) = connection.create_request_stream().await?;

        // Separate pseudo-headers from regular headers
        let mut pseudo_headers = vec![];
        let mut regular_headers = vec![];

        if let Some(headers) = &headers {
            for header in headers {
                // Normalize header names to lowercase for HTTP/3 compliance
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
        let mut h3_headers = pseudo_headers;
        h3_headers.extend(regular_headers);

        // Validate headers before sending
        for header in &h3_headers {
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
        let headers_frame =
            crate::types::Frame::headers_h3(stream_id, &h3_headers).map_err(|e| {
                ProtocolError::H3MessageError(format!("Failed to create headers frame: {}", e))
            })?;
        let serialized_headers = headers_frame.serialize_h3().map_err(|e| {
            ProtocolError::H3MessageError(format!("Failed to serialize headers: {}", e))
        })?;

        send_stream
            .write_all(&serialized_headers)
            .await
            .map_err(|e| ProtocolError::H3StreamError(format!("Failed to send headers: {}", e)))?;

        // Send DATA frame if body exists
        if let Some(body) = body {
            if !body.is_empty() {
                let data_frame = crate::types::Frame::data_h3(stream_id, body.clone());
                let serialized_data = data_frame.serialize_h3().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize data: {}", e))
                })?;

                send_stream.write_all(&serialized_data).await.map_err(|e| {
                    ProtocolError::H3StreamError(format!("Failed to send data: {}", e))
                })?;
            }
        }

        // Send trailers as additional HEADERS frame if they exist
        if let Some(trailers) = trailers {
            if !trailers.is_empty() {
                let trailers_frame = crate::types::Frame::headers_h3(stream_id, &trailers)
                    .map_err(|e| {
                        ProtocolError::H3MessageError(format!(
                            "Failed to create trailers frame: {}",
                            e
                        ))
                    })?;
                let serialized_trailers = trailers_frame.serialize_h3().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize trailers: {}", e))
                })?;

                send_stream
                    .write_all(&serialized_trailers)
                    .await
                    .map_err(|e| {
                        ProtocolError::H3StreamError(format!("Failed to send trailers: {}", e))
                    })?;
            }
        }

        // Finish sending (half-close the send side)
        send_stream
            .finish()
            .map_err(|e| ProtocolError::H3StreamError(format!("Failed to finish stream: {}", e)))?;

        Ok(stream_id)
    }

    async fn get_response(
        &self,
        connection: &mut H3Connection,
        stream_id: u32,
    ) -> Result<Response, ProtocolError> {
        let mut status = 0u16;
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut trailers = None;
        let protocol_version = "HTTP/3.0".to_string();
        let mut headers_received = false;

        // Read frames from the specific request stream
        let mut data_received = false;

        loop {
            let frame = connection.read_request_frame(stream_id).await?;

            match &frame.frame_type {
                FrameType::H3(FrameTypeH3::Headers) => {
                    let decoded_headers = frame.decode_headers_h3()?;

                    if !headers_received {
                        // First HEADERS frame contains response headers
                        for header in &decoded_headers {
                            if header.name == ":status" {
                                if let Some(ref status_str) = header.value {
                                    status = status_str.parse::<u16>().unwrap_or(500);
                                }
                            } else if !header.name.starts_with(':') {
                                headers.push(header.clone());
                            }
                        }
                        headers_received = true;
                    } else {
                        // Subsequent HEADERS frames are trailers
                        let mut trailer_headers = Vec::new();
                        for header in &decoded_headers {
                            if !header.name.starts_with(':') {
                                trailer_headers.push(header.clone());
                            }
                        }
                        if !trailer_headers.is_empty() {
                            trailers = Some(trailer_headers);
                        }
                    }

                    connection.handle_frame(&frame).await?;

                    // Continue reading for DATA frames after first HEADERS
                    if !headers_received {
                        continue;
                    }
                }
                FrameType::H3(FrameTypeH3::Data) => {
                    body.extend_from_slice(&frame.payload);
                    connection.handle_frame(&frame).await?;
                    data_received = true;
                    // Continue reading to check for trailers
                }
                FrameType::H3(FrameTypeH3::Settings) => {
                    connection.handle_frame(&frame).await?;
                    // Continue reading for actual response frames
                }
                _ => {
                    connection.handle_frame(&frame).await?;
                }
            }

            // Break if we have headers and either data or reached end of stream
            if headers_received && (data_received || frame.payload.is_empty()) {
                // Check for one more frame to see if there are trailers
                match connection.read_request_frame(stream_id).await {
                    Ok(next_frame) => {
                        if let FrameType::H3(FrameTypeH3::Headers) = &next_frame.frame_type {
                            // This is a trailers frame
                            let decoded_trailers = next_frame.decode_headers_h3()?;
                            let mut trailer_headers = Vec::new();
                            for header in &decoded_trailers {
                                if !header.name.starts_with(':') {
                                    trailer_headers.push(header.clone());
                                }
                            }
                            if !trailer_headers.is_empty() {
                                trailers = Some(trailer_headers);
                            }
                        }
                        connection.handle_frame(&next_frame).await?;
                    }
                    Err(_) => {
                        // No more frames, end of stream
                    }
                }
                break;
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

    pub async fn send_str(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<&str>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(|s| Bytes::from(s.to_string()));
        self.send_bytes(target, method, headers, body_bytes, trailers)
            .await
    }

    pub async fn get_str(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<&str>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(|s| Bytes::from(s.to_string()));
        self.get_bytes(target, headers, body_bytes, trailers).await
    }

    pub async fn post_str(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<&str>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(|s| Bytes::from(s.to_string()));
        self.post_bytes(target, headers, body_bytes, trailers).await
    }

    pub async fn send_string(
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

    pub async fn get_string(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(Bytes::from);
        self.get_bytes(target, headers, body_bytes, trailers).await
    }

    pub async fn post_string(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(Bytes::from);
        self.post_bytes(target, headers, body_bytes, trailers).await
    }
}

#[async_trait]
impl Protocol for H3Client {
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

        // Create and establish HTTP/3 connection
        let mut connection = H3Connection::connect(target).await?;

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
