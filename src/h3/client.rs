use crate::h3::connection::H3Connection;
use crate::types::{
    ClientTimeouts, FrameType, FrameTypeH3, H3StreamErrorKind, Header, Protocol, ProtocolError,
    Request, Response,
};
use crate::utils::{
    ensure_user_agent, merge_headers, normalize_headers, prepare_pseudo_headers,
    with_timeout_result, HTTP_VERSION_3_0,
};
use async_trait::async_trait;
use bytes::Bytes;

pub struct H3Client {
    timeouts: ClientTimeouts,
}

impl H3Client {
    pub fn new() -> Self {
        Self::with_timeouts(ClientTimeouts::default())
    }

    pub fn with_timeouts(timeouts: ClientTimeouts) -> Self {
        Self { timeouts }
    }

    pub fn timeouts(&self) -> &ClientTimeouts {
        &self.timeouts
    }

    pub fn build_request(
        target: &str,
        method: impl Into<String>,
        headers: Vec<Header>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Request, ProtocolError> {
        Request::new(target, method).map(|r| {
            r.with_headers(headers)
                .with_optional_body(body)
                .with_trailers(trailers)
        })
    }

    async fn send_request_inner(
        &self,
        connection: &mut H3Connection,
        request: &Request,
    ) -> Result<u32, ProtocolError> {
        let (stream_id, mut send_stream) =
            with_timeout_result(self.timeouts.connect, connection.create_request_stream()).await?;

        let pseudo_headers = prepare_pseudo_headers(request, &request.target)?;
        let mut headers = merge_headers(pseudo_headers, request);
        ensure_user_agent(&mut headers);

        let header_block = with_timeout_result(
            self.timeouts.write,
            connection.encode_headers(stream_id, &headers),
        )
        .await?;
        let headers_frame =
            crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, header_block);
        let serialized_headers = headers_frame.serialize().map_err(|e| {
            ProtocolError::H3MessageError(format!("Failed to serialize headers: {}", e))
        })?;
        with_timeout_result(self.timeouts.write, async {
            send_stream
                .write_all(&serialized_headers)
                .await
                .map_err(|e| {
                    ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                        "Failed to send headers: {}",
                        e
                    )))
                })
        })
        .await?;

        if let Some(body) = request.body.as_ref() {
            if !body.is_empty() {
                let data_frame = crate::types::FrameH3::data(stream_id, body.clone());
                let serialized_data = data_frame.serialize().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize data: {}", e))
                })?;
                with_timeout_result(self.timeouts.write, async {
                    send_stream.write_all(&serialized_data).await.map_err(|e| {
                        ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                            "Failed to send data: {}",
                            e
                        )))
                    })
                })
                .await?;
            }
        }

        if let Some(trailers) = request.trailers.as_ref() {
            if !trailers.is_empty() {
                let normalized_trailers = normalize_headers(trailers);
                let trailer_block = with_timeout_result(
                    self.timeouts.write,
                    connection.encode_headers(stream_id, &normalized_trailers),
                )
                .await?;
                let trailers_frame =
                    crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, trailer_block);
                let serialized_trailers = trailers_frame.serialize().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize trailers: {}", e))
                })?;
                with_timeout_result(self.timeouts.write, async {
                    send_stream
                        .write_all(&serialized_trailers)
                        .await
                        .map_err(|e| {
                            ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(
                                format!("Failed to send trailers: {}", e),
                            ))
                        })
                })
                .await?;
            }
        }

        with_timeout_result(self.timeouts.write, async {
            send_stream.finish().map_err(|e| {
                ProtocolError::H3StreamError(H3StreamErrorKind::ProtocolViolation(format!(
                    "Failed to finish stream: {}",
                    e
                )))
            })
        })
        .await?;

        Ok(stream_id)
    }

    pub async fn send_request(&self, request: Request) -> Result<Response, ProtocolError> {
        let mut connection = with_timeout_result(
            self.timeouts.connect,
            H3Connection::connect(&request.target),
        )
        .await?;
        let stream_id = self.send_request_inner(&mut connection, &request).await?;
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
        let protocol_version = HTTP_VERSION_3_0.to_string();

        loop {
            with_timeout_result(self.timeouts.read, connection.poll_control()).await?;
            let frame_opt =
                with_timeout_result(self.timeouts.read, connection.read_request_frame(stream_id))
                    .await?;

            let frame = match frame_opt {
                Some(frame) => frame,
                None => break,
            };

            match &frame.frame_type {
                FrameType::H3(FrameTypeH3::Headers) => {
                    let decoded_headers = with_timeout_result(
                        self.timeouts.read,
                        connection.decode_headers(stream_id, &frame.payload),
                    )
                    .await?;

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
                    with_timeout_result(self.timeouts.read, connection.handle_frame(&frame))
                        .await?;
                }
                _ => {
                    with_timeout_result(self.timeouts.read, connection.handle_frame(&frame))
                        .await?;
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
    async fn send(&self, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(request).await
    }
}
