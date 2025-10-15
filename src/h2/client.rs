use crate::h2::connection::{H2Connection, StreamEvent};
use crate::types::{H2StreamErrorKind, Header, Protocol, ProtocolError, Request, Response, Target};
use crate::utils::{merge_headers, normalize_headers, prepare_pseudo_headers};
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

    async fn send_request_inner(
        &self,
        connection: &mut H2Connection,
        target: &Target,
        request: &Request,
    ) -> Result<u32, ProtocolError> {
        let stream_id = connection.create_stream().await?;

        let pseudo_headers = prepare_pseudo_headers(request, target)?;
        let headers = merge_headers(pseudo_headers, request);

        let has_body = request.body.as_ref().map_or(false, |body| !body.is_empty());
        let has_trailers = request
            .trailers
            .as_ref()
            .map_or(false, |trailers| !trailers.is_empty());

        let end_stream = !has_body && !has_trailers;
        connection
            .send_headers(stream_id, &headers, end_stream)
            .await
            .map_err(|e| {
                ProtocolError::H2StreamError(H2StreamErrorKind::ProtocolViolation(format!(
                    "Failed to send headers: {}",
                    e
                )))
            })?;

        if let Some(body) = request.body.as_ref() {
            if !body.is_empty() {
                let end_stream = !has_trailers;
                connection
                    .send_data(stream_id, body, end_stream)
                    .await
                    .map_err(|e| {
                        ProtocolError::H2StreamError(H2StreamErrorKind::ProtocolViolation(format!(
                            "Failed to send data: {}",
                            e
                        )))
                    })?;
            }
        }

        if let Some(trailers) = request.trailers.as_ref() {
            if !trailers.is_empty() {
                let normalized_trailers = normalize_headers(trailers);
                connection
                    .send_headers(stream_id, &normalized_trailers, true)
                    .await
                    .map_err(|e| {
                        ProtocolError::H2StreamError(H2StreamErrorKind::ProtocolViolation(format!(
                            "Failed to send trailers: {}",
                            e
                        )))
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
        stream_id: u32,
    ) -> Result<Response, ProtocolError> {
        let protocol_version = "HTTP/2.0".to_string();
        let mut status: Option<u16> = None;
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut trailers: Option<Vec<Header>> = None;

        loop {
            let event = connection.recv_stream_event(stream_id).await?;
            match event {
                StreamEvent::Headers {
                    headers: block,
                    end_stream,
                    is_trailer,
                } => {
                    if !is_trailer {
                        let mut parsed_status: Option<u16> = None;
                        let mut filtered = Vec::new();
                        for header in block.into_iter() {
                            if header.name == ":status" {
                                if let Some(ref value) = header.value {
                                    if let Ok(code) = value.parse::<u16>() {
                                        parsed_status = Some(code);
                                    }
                                }
                            } else if !header.name.starts_with(':') {
                                filtered.push(header);
                            }
                        }

                        let code = parsed_status.ok_or_else(|| {
                            ProtocolError::InvalidResponse(
                                "Missing :status header in response".to_string(),
                            )
                        })?;

                        if code < 200 {
                            if end_stream {
                                return Err(ProtocolError::InvalidResponse(
                                    "Informational response closed stream".to_string(),
                                ));
                            }
                            continue;
                        }

                        status = Some(code);
                        headers = filtered;

                        if end_stream {
                            break;
                        }
                    } else {
                        let trailer_headers = trailers.get_or_insert_with(Vec::new);
                        trailer_headers
                            .extend(block.into_iter().filter(|h| !h.name.starts_with(':')));
                        if end_stream {
                            break;
                        }
                    }
                }
                StreamEvent::Data {
                    payload,
                    end_stream,
                } => {
                    body.extend_from_slice(&payload);
                    if end_stream {
                        break;
                    }
                }
                StreamEvent::RstStream { error_code } => {
                    return Err(ProtocolError::H2StreamError(H2StreamErrorKind::Reset(
                        error_code,
                    )));
                }
            }
        }

        let status = status.ok_or_else(|| {
            ProtocolError::InvalidResponse("No final response received".to_string())
        })?;

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
impl Protocol for H2Client {
    async fn send(&self, target: &Target, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(target, request).await
    }
}
