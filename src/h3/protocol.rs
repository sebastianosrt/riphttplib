use crate::PreparedRequest;
use crate::h3::connection::H3Connection;
use crate::types::{
    ClientTimeouts, FrameType, FrameTypeH3, H3StreamErrorKind, Header, Protocol, ProtocolError,
    Request, Response, ResponseFrame,
};
use crate::utils::{apply_redirect, timeout_result, HTTP_VERSION_3_0};
use async_trait::async_trait;
use bytes::Bytes;

#[derive(Clone)]
pub struct H3 {
    timeouts: ClientTimeouts,
}

impl H3 {
    pub fn new() -> Self {
        Self::timeouts(ClientTimeouts::default())
    }

    pub fn timeouts(timeouts: ClientTimeouts) -> Self {
        Self { timeouts }
    }

    pub fn get_timeouts(&self) -> &ClientTimeouts {
        &self.timeouts
    }

    pub fn session(&self) -> crate::session::H3Session {
        crate::session::H3Session::new(self.clone())
    }

    pub fn build_request(
        target: &str,
        method: impl Into<String>,
        headers: Vec<Header>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<PreparedRequest, ProtocolError> {
        let mut request = Request::new(target, method)?;
        request.headers_mut(headers);
        if let Some(body_bytes) = body {
            request.set_body(body_bytes);
        }
        if let Some(trailer_headers) = trailers {
            request.trailers_mut(trailer_headers);
        }
        request.prepare_request()
    }

    async fn send_request_inner(
        &self,
        connection: &mut H3Connection,
        request: &Request,
        timeouts: &ClientTimeouts,
    ) -> Result<u32, ProtocolError> {
        let (stream_id, mut send_stream) =
            timeout_result(timeouts.connect, connection.create_request_stream()).await?;

        let prepared = request.prepare_request()?;
        let mut header_block_entries = prepared.pseudo_headers.clone();
        let mut normalized_headers = prepared.headers.clone();
        for header in &mut normalized_headers {
            header.normalize();
        }
        header_block_entries.extend(normalized_headers);

        let header_block = timeout_result(
            timeouts.write,
            connection.encode_headers(stream_id, &header_block_entries),
        )
        .await?;
        let headers_frame =
            crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, header_block);
        let serialized_headers = headers_frame.serialize().map_err(|e| {
            ProtocolError::H3MessageError(format!("Failed to serialize headers: {}", e))
        })?;
        timeout_result(timeouts.write, async {
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

        if let Some(body) = prepared.body.as_ref() {
            if !body.is_empty() {
                let data_frame = crate::types::FrameH3::data(stream_id, body.clone());
                let serialized_data = data_frame.serialize().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize data: {}", e))
                })?;
                timeout_result(timeouts.write, async {
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

        if !prepared.trailers.is_empty() {
            let trailer_block = timeout_result(
                timeouts.write,
                connection.encode_headers(stream_id, &prepared.trailers),
            )
            .await?;
            let trailers_frame =
                crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, trailer_block);
            let serialized_trailers = trailers_frame.serialize().map_err(|e| {
                ProtocolError::H3MessageError(format!("Failed to serialize trailers: {}", e))
            })?;
            timeout_result(timeouts.write, async {
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

        timeout_result(timeouts.write, async {
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
        self.send_request_internal(request, 0).await
    }

    fn send_request_internal<'a>(
        &'a self,
        mut request: Request,
        redirect_count: u32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, ProtocolError>> + 'a>>
    {
        Box::pin(async move {
            const MAX_REDIRECTS: u32 = 30;

            if redirect_count > MAX_REDIRECTS {
                return Err(ProtocolError::RequestFailed(
                    "Too many redirects".to_string(),
                ));
            }

            let timeouts = request.timeouts(&self.timeouts);
            let mut connection = timeout_result(
                timeouts.connect,
                H3Connection::connect_with_target(&request.target),
            )
            .await?;
            let stream_id = self
                .send_request_inner(&mut connection, &request, &timeouts)
                .await?;
            let response = self
                .read_response(&mut connection, stream_id, &timeouts)
                .await?;

            if apply_redirect(&mut request, &response)? {
                return self
                    .send_request_internal(request, redirect_count + 1)
                    .await;
            }

            Ok(response)
        })
    }

    async fn read_response(
        &self,
        connection: &mut H3Connection,
        stream_id: u32,
        timeouts: &ClientTimeouts,
    ) -> Result<Response, ProtocolError> {
        let mut status: Option<u16> = None;
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut trailers: Option<Vec<Header>> = None;
        let mut headers_received = false;
        let protocol = HTTP_VERSION_3_0.to_string();
        let mut captured_frames = Vec::new();

        loop {
            timeout_result(timeouts.read, connection.poll_control()).await?;
            let frame_opt =
                timeout_result(timeouts.read, connection.read_request_frame(stream_id)).await?;

            let frame = match frame_opt {
                Some(frame) => frame,
                None => {
                    // Stream has ended, mark it as finished receiving
                    let _ = connection.stream_finished_receiving(stream_id);
                    break;
                }
            };

            captured_frames.push(ResponseFrame::Http3(frame.clone()));

            match &frame.frame_type {
                FrameType::H3(FrameTypeH3::Headers) => {
                    let decoded_headers = timeout_result(
                        timeouts.read,
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
                                timeout_result(timeouts.read, connection.handle_frame(&frame))
                                    .await?;
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

                    timeout_result(timeouts.read, connection.handle_frame(&frame)).await?;
                }
                FrameType::H3(FrameTypeH3::Data) => {
                    body.extend_from_slice(&frame.payload);
                    timeout_result(timeouts.read, connection.handle_frame(&frame)).await?;
                }
                _ => {
                    timeout_result(timeouts.read, connection.handle_frame(&frame)).await?;
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

        // Clean up the closed stream
        connection.remove_closed_stream(stream_id);

        let cookies = Response::collect_cookies(&headers);

        Ok(Response {
            status,
            protocol,
            headers,
            body: Bytes::from(body),
            trailers,
            frames: if captured_frames.is_empty() {
                None
            } else {
                Some(captured_frames)
            },
            cookies,
        })
    }
}

#[async_trait(?Send)]
impl Protocol for H3 {
    async fn response(&self, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(request).await
    }
}
