use crate::h2::connection::H2Connection;
use crate::types::{
    FrameType, FrameTypeH2, Header, Protocol, ProtocolError, Request, Response, Target,
};
use async_trait::async_trait;
use bytes::Bytes;

pub struct H2Client;

impl H2Client {
    pub fn new() -> Self {
        Self
    }

    pub fn build_request(
        method: impl Into<String>,
        headers: Vec<Header>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Request {
        Request::new(method)
            .with_headers(headers)
            .with_optional_body(body)
            .with_trailers(trailers)
    }

    fn normalize_headers(headers: &[Header]) -> Vec<Header> {
        headers
            .iter()
            .map(|h| Header {
                name: h.name.to_lowercase(),
                value: h.value.clone(),
            })
            .collect()
    }

    fn prepare_pseudo_headers(
        request: &Request,
        target: &Target,
    ) -> Result<Vec<Header>, ProtocolError> {
        let mut pseudo_headers: Vec<Header> = request
            .headers
            .iter()
            .filter(|h| h.name.starts_with(':'))
            .cloned()
            .collect();

        if !pseudo_headers.iter().any(|h| h.name == ":method") {
            pseudo_headers.insert(
                0,
                Header::new(":method".to_string(), request.method.clone()),
            );
        }

        let method = request.method.to_uppercase();
        match method.as_str() {
            "CONNECT" => {
                if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                    let authority = target.authority().ok_or_else(|| {
                        ProtocolError::InvalidTarget(
                            "CONNECT requests require an authority".to_string(),
                        )
                    })?;
                    pseudo_headers.push(Header::new(":authority".to_string(), authority));
                }
                pseudo_headers.retain(|h| h.name != ":scheme" && h.name != ":path");
            }
            "OPTIONS" => {
                let path_value = if target.path_only() == "*" {
                    "*".to_string()
                } else {
                    target.path()
                };
                if !pseudo_headers.iter().any(|h| h.name == ":path") {
                    pseudo_headers.push(Header::new(":path".to_string(), path_value));
                }
                if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                    if let Some(authority) = target.authority() {
                        pseudo_headers.push(Header::new(":authority".to_string(), authority));
                    }
                }
                if !pseudo_headers.iter().any(|h| h.name == ":scheme") {
                    pseudo_headers.push(Header::new(
                        ":scheme".to_string(),
                        target.scheme().to_string(),
                    ));
                }
            }
            _ => {
                if !pseudo_headers.iter().any(|h| h.name == ":path") {
                    pseudo_headers.push(Header::new(":path".to_string(), target.path()));
                }
                if !pseudo_headers.iter().any(|h| h.name == ":scheme") {
                    pseudo_headers.push(Header::new(
                        ":scheme".to_string(),
                        target.scheme().to_string(),
                    ));
                }
                if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                    if let Some(authority) = target.authority() {
                        pseudo_headers.push(Header::new(":authority".to_string(), authority));
                    }
                }
            }
        }

        Ok(Self::normalize_headers(&pseudo_headers))
    }

    fn merge_headers(pseudo: Vec<Header>, request: &Request) -> Vec<Header> {
        let mut headers = Vec::with_capacity(pseudo.len() + request.headers.len());
        headers.extend(pseudo);
        headers.extend(
            request
                .headers
                .iter()
                .filter(|h| !h.name.starts_with(':'))
                .cloned()
                .map(|mut h| {
                    h.name = h.name.to_lowercase();
                    h
                }),
        );
        headers
    }

    fn validate_headers(headers: &[Header]) -> Result<(), ProtocolError> {
        for header in headers {
            if header.name.is_empty() {
                return Err(ProtocolError::MalformedHeaders(
                    "Empty header name".to_string(),
                ));
            }
            if header.name.chars().any(|c| c.is_whitespace()) {
                return Err(ProtocolError::MalformedHeaders(format!(
                    "Header name contains whitespace: {}",
                    header.name
                )));
            }
        }
        Ok(())
    }

    async fn send_request_inner(
        &self,
        connection: &mut H2Connection,
        target: &Target,
        request: &Request,
    ) -> Result<u32, ProtocolError> {
        let stream_id = connection.create_stream().await?;

        let pseudo_headers = Self::prepare_pseudo_headers(request, target)?;
        let headers = Self::merge_headers(pseudo_headers, request);
        Self::validate_headers(&headers)?;

        let has_body = request.body.as_ref().map_or(false, |body| !body.is_empty());
        let has_trailers = request
            .trailers
            .as_ref()
            .map_or(false, |trailers| !trailers.is_empty());

        let end_stream = !has_body && !has_trailers;
        connection
            .send_headers(stream_id, &headers, end_stream)
            .await
            .map_err(|e| ProtocolError::H2StreamError(format!("Failed to send headers: {}", e)))?;

        if let Some(body) = request.body.as_ref() {
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

        if let Some(trailers) = request.trailers.as_ref() {
            if !trailers.is_empty() {
                let normalized_trailers = Self::normalize_headers(trailers);
                connection
                    .send_headers(stream_id, &normalized_trailers, true)
                    .await
                    .map_err(|e| {
                        ProtocolError::H2StreamError(format!("Failed to send trailers: {}", e))
                    })?;
            }
        }

        Ok(stream_id)
    }

    pub async fn send_request(
        &self,
        target: &Target,
        request: Request,
    ) -> Result<Response, ProtocolError> {
        let mut connection = H2Connection::connect(target).await?;
        let stream_id = self
            .send_request_inner(&mut connection, target, &request)
            .await?;
        self.read_response(&mut connection, stream_id).await
    }

    async fn read_response(
        &self,
        connection: &mut H2Connection,
        _stream_id: u32,
    ) -> Result<Response, ProtocolError> {
        let mut status = 0u16;
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut trailers = Vec::new();
        let protocol_version = "HTTP/2.0".to_string();
        let mut headers_received = false;

        loop {
            let frame = connection.read_frame().await?;

            match &frame.frame_type {
                FrameType::H2(FrameTypeH2::Headers) => {
                    let decoded_headers = frame.decode_headers()?;

                    if !headers_received {
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
                        for header in &decoded_headers {
                            if !header.name.starts_with(':') {
                                trailers.push(header.clone());
                            }
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
                    if headers_received {
                        for header in &decoded_headers {
                            if !header.name.starts_with(':') {
                                trailers.push(header.clone());
                            }
                        }
                    } else {
                        for header in &decoded_headers {
                            if !header.name.starts_with(':') {
                                headers.push(header.clone());
                            }
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
            trailers: if trailers.is_empty() {
                None
            } else {
                Some(trailers)
            },
        })
    }
}

#[async_trait]
impl Protocol for H2Client {
    async fn send(&self, target: &Target, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(target, request).await
    }
}
