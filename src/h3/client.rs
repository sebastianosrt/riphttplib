use crate::h3::connection::H3Connection;
use crate::types::{
    FrameType, FrameTypeH3, H3StreamErrorKind, Header, Protocol, ProtocolError, Request, Response,
    Target,
};
use async_trait::async_trait;
use bytes::Bytes;

pub struct H3Client;

impl H3Client {
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
                let path = if target.path_only() == "*" {
                    "*".to_string()
                } else {
                    target.path()
                };
                if !pseudo_headers.iter().any(|h| h.name == ":path") {
                    pseudo_headers.push(Header::new(":path".to_string(), path));
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

    async fn send_request_inner(
        &self,
        connection: &mut H3Connection,
        target: &Target,
        request: &Request,
    ) -> Result<u32, ProtocolError> {
        let (stream_id, mut send_stream) = connection.create_request_stream().await?;

        let pseudo_headers = Self::prepare_pseudo_headers(request, target)?;
        let headers = Self::merge_headers(pseudo_headers, request);

        let header_block = connection.encode_headers(stream_id, &headers).await?;
        let headers_frame =
            crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, header_block);
        let serialized_headers = headers_frame.serialize().map_err(|e| {
            ProtocolError::H3MessageError(format!("Failed to serialize headers: {}", e))
        })?;
        send_stream
            .write_all(&serialized_headers)
            .await
            .map_err(|e| {
                ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                    "Failed to send headers: {}",
                    e
                )))
            })?;

        if let Some(body) = request.body.as_ref() {
            if !body.is_empty() {
                let data_frame = crate::types::FrameH3::data(stream_id, body.clone());
                let serialized_data = data_frame.serialize().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize data: {}", e))
                })?;
                send_stream.write_all(&serialized_data).await.map_err(|e| {
                    ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                        "Failed to send data: {}",
                        e
                    )))
                })?;
            }
        }

        if let Some(trailers) = request.trailers.as_ref() {
            if !trailers.is_empty() {
                let normalized_trailers = Self::normalize_headers(trailers);
                let trailer_block = connection
                    .encode_headers(stream_id, &normalized_trailers)
                    .await?;
                let trailers_frame =
                    crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, trailer_block);
                let serialized_trailers = trailers_frame.serialize().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize trailers: {}", e))
                })?;
                send_stream
                    .write_all(&serialized_trailers)
                    .await
                    .map_err(|e| {
                        ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                            "Failed to send trailers: {}",
                            e
                        )))
                    })?;
            }
        }

        send_stream.finish().map_err(|e| {
            ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                "Failed to finish stream: {}",
                e
            )))
        })?;

        Ok(stream_id)
    }

    pub async fn send_request(
        &self,
        target: &Target,
        request: Request,
    ) -> Result<Response, ProtocolError> {
        let mut connection = H3Connection::connect(target).await?;
        let stream_id = self
            .send_request_inner(&mut connection, target, &request)
            .await?;
        self.read_response(&mut connection, stream_id).await
    }

    async fn read_response(
        &self,
        connection: &mut H3Connection,
        stream_id: u32,
    ) -> Result<Response, ProtocolError> {
        let mut status: Option<u16> = None;
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut trailers: Option<Vec<Header>> = None;
        let mut headers_received = false;
        let protocol_version = "HTTP/3.0".to_string();

        loop {
            connection.poll_control().await?;
            let frame_opt = connection.read_request_frame(stream_id).await?;

            let frame = match frame_opt {
                Some(frame) => frame,
                None => break,
            };

            match &frame.frame_type {
                FrameType::H3(FrameTypeH3::Headers) => {
                    let decoded_headers =
                        connection.decode_headers(stream_id, &frame.payload).await?;

                    let mut status_code = decoded_headers.iter().find_map(|header| {
                        (header.name == ":status")
                            .then(|| header.value.as_ref()?.parse::<u16>().ok())
                            .flatten()
                    });

                    if !headers_received {
                        let code = status_code.take().ok_or_else(|| {
                            ProtocolError::InvalidResponse(
                                "Missing :status header in response".to_string(),
                            )
                        })?;
                        if code < 200 {
                            connection.handle_frame(&frame).await?;
                            continue;
                        }
                        status = Some(code);
                        headers.extend(
                            decoded_headers
                                .iter()
                                .filter(|h| !h.name.starts_with(':'))
                                .cloned(),
                        );
                        headers_received = true;
                    } else {
                        if let Some(code) = status_code {
                            if code < 200 {
                                connection.handle_frame(&frame).await?;
                                continue;
                            }
                        }
                        let trailer_headers = trailers.get_or_insert_with(Vec::new);
                        trailer_headers.extend(
                            decoded_headers
                                .iter()
                                .filter(|h| !h.name.starts_with(':'))
                                .cloned(),
                        );
                    }

                    connection.handle_frame(&frame).await?;
                }
                FrameType::H3(FrameTypeH3::Data) => {
                    body.extend_from_slice(&frame.payload);
                    connection.handle_frame(&frame).await?;
                }
                _ => {
                    connection.handle_frame(&frame).await?;
                }
            }
        }

        let status = status.ok_or_else(|| {
            ProtocolError::InvalidResponse("No final response received".to_string())
        })?;

        let trailers = match trailers {
            Some(t) if !t.is_empty() => Some(t),
            _ => None,
        };

        Ok(Response {
            status,
            protocol_version,
            headers,
            body: Bytes::from(body),
            trailers,
        })
    }
}

#[async_trait(?Send)]
impl Protocol for H3Client {
    async fn send(&self, target: &Target, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(target, request).await
    }
}
