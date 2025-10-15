use crate::h2::connection::{H2Connection, StreamEvent};
use crate::types::{
    ClientTimeouts, H2StreamErrorKind, Header, Protocol, ProtocolError, Request, Response,
};
use crate::utils::{ensure_user_agent, merge_headers, normalize_headers, prepare_pseudo_headers, parse_target};
use async_trait::async_trait;
use bytes::Bytes;
use url::Url;

pub struct H2Client {
    timeouts: ClientTimeouts,
}

impl H2Client {
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
        connection: &mut H2Connection,
        request: &Request,
    ) -> Result<u32, ProtocolError> {
        let stream_id = connection.create_stream().await?;

        let pseudo_headers = prepare_pseudo_headers(request, &request.target)?;
        let mut headers = merge_headers(pseudo_headers, request);
        ensure_user_agent(&mut headers);

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
            let mut connection = H2Connection::connect(&request.target, &timeouts).await?;
            let stream_id = self.send_request_inner(&mut connection, &request).await?;
            let response = self.read_response(&mut connection, stream_id, request.stream).await?;

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
        connection: &mut H2Connection,
        stream_id: u32,
        stream_response: bool,
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

                    // For streaming, return after reading the first data frame
                    if stream_response && !body.is_empty() {
                        break;
                    }

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
impl Protocol for H2Client {
    async fn send(&self, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(request).await
    }
}
