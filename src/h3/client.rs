use crate::h3::connection::H3Connection;
use crate::types::{
    ClientTimeouts, FrameType, FrameTypeH3, H3StreamErrorKind, Header, Protocol, ProtocolError,
    Request, Response,
};
use crate::utils::{
    ensure_user_agent, merge_headers, normalize_headers, prepare_pseudo_headers,
    with_timeout_result, HTTP_VERSION_3_0, parse_target,
};
use async_trait::async_trait;
use bytes::Bytes;
use url::Url;

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
        timeouts: &ClientTimeouts,
    ) -> Result<u32, ProtocolError> {
        let (stream_id, mut send_stream) =
            with_timeout_result(timeouts.connect, connection.create_request_stream()).await?;

        let pseudo_headers = prepare_pseudo_headers(request, &request.target)?;
        let mut headers = merge_headers(pseudo_headers, request);
        ensure_user_agent(&mut headers);

        let header_block = with_timeout_result(
            timeouts.write,
            connection.encode_headers(stream_id, &headers),
        )
        .await?;
        let headers_frame =
            crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, header_block);
        let serialized_headers = headers_frame.serialize().map_err(|e| {
            ProtocolError::H3MessageError(format!("Failed to serialize headers: {}", e))
        })?;
        with_timeout_result(timeouts.write, async {
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
                with_timeout_result(timeouts.write, async {
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
                    timeouts.write,
                    connection.encode_headers(stream_id, &normalized_trailers),
                )
                .await?;
                let trailers_frame =
                    crate::types::FrameH3::new(FrameTypeH3::Headers, stream_id, trailer_block);
                let serialized_trailers = trailers_frame.serialize().map_err(|e| {
                    ProtocolError::H3MessageError(format!("Failed to serialize trailers: {}", e))
                })?;
                with_timeout_result(timeouts.write, async {
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

        with_timeout_result(timeouts.write, async {
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

    fn send_request_internal<'a>(&'a self, mut request: Request, redirect_count: u32) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, ProtocolError>> + 'a>> {
        Box::pin(async move {
            const MAX_REDIRECTS: u32 = 30;

            if redirect_count > MAX_REDIRECTS {
                return Err(ProtocolError::RequestFailed("Too many redirects".to_string()));
            }

            let timeouts = request.effective_timeouts(&self.timeouts);
            let mut connection = with_timeout_result(timeouts.connect, H3Connection::connect(&request.target)).await?;
            let stream_id = self.send_request_inner(&mut connection, &request, &timeouts).await?;
            let response = self.read_response(&mut connection, stream_id, &timeouts, request.stream).await?;

            // Handle redirects if enabled
            if request.allow_redirects && Self::is_redirect_status(response.status) {
                if let Some(location) = Self::get_location_header(&response.headers) {
                    if let Ok(redirect_url) = Self::resolve_redirect_url(&request.target.url, &location) {
                        let new_target = parse_target(redirect_url.as_str())?;
                        request.target = new_target;

                        // For 303 responses or GET/HEAD methods on 301/302, change method to GET
                        if response.status == 303 ||
                           ((response.status == 301 || response.status == 302) &&
                            (request.method == "GET" || request.method == "HEAD")) {
                            request.method = "GET".to_string();
                            request.body = None;
                            request.json = None;
                        }

                        return self.send_request_internal(request, redirect_count + 1).await;
                    }
                }
            }

            Ok(response)
        })
    }

    async fn read_response(
        &self,
        connection: &mut H3Connection,
        stream_id: u32,
        timeouts: &ClientTimeouts,
        stream_response: bool,
    ) -> Result<Response, ProtocolError> {
        let mut status: Option<u16> = None;
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut trailers: Option<Vec<Header>> = None;
        let mut headers_received = false;
        let protocol = HTTP_VERSION_3_0.to_string();

        loop {
            with_timeout_result(timeouts.read, connection.poll_control()).await?;
            let frame_opt = with_timeout_result(
                timeouts.read,
                connection.read_request_frame(stream_id),
            )
            .await?;

            let frame = match frame_opt {
                Some(frame) => frame,
                None => {
                    // Stream has ended, mark it as finished receiving
                    let _ = connection.stream_finished_receiving(stream_id);
                    break;
                }
            };

            match &frame.frame_type {
                FrameType::H3(FrameTypeH3::Headers) => {
                    let decoded_headers = with_timeout_result(
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
                                with_timeout_result(timeouts.read, connection.handle_frame(&frame))
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

                    with_timeout_result(timeouts.read, connection.handle_frame(&frame)).await?;
                }
                FrameType::H3(FrameTypeH3::Data) => {
                    body.extend_from_slice(&frame.payload);
                    with_timeout_result(timeouts.read, connection.handle_frame(&frame)).await?;

                    // For streaming, return after reading the first data frame
                    if stream_response && !body.is_empty() {
                        break;
                    }
                }
                _ => {
                    with_timeout_result(timeouts.read, connection.handle_frame(&frame)).await?;
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

        Ok(Response {
            status,
            protocol,
            headers,
            body: Bytes::from(body),
            trailers,
        })
    }

    fn is_redirect_status(status: u16) -> bool {
        matches!(status, 300..=399)
    }

    fn get_location_header(headers: &[Header]) -> Option<String> {
        headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("location"))
            .and_then(|h| h.value.clone())
    }

    fn resolve_redirect_url(base_url: &Url, location: &str) -> Result<Url, url::ParseError> {
        if location.starts_with("http://") || location.starts_with("https://") {
            Url::parse(location)
        } else {
            base_url.join(location)
        }
    }
}

#[async_trait(?Send)]
impl Protocol for H3Client {
    async fn send(&self, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(request).await
    }
}
