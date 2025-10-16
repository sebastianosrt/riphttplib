use crate::h2::connection::H2Connection;
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
        Self::timeouts(ClientTimeouts::default())
    }

    pub fn timeouts(timeouts: ClientTimeouts) -> Self {
        Self { timeouts }
    }

    pub fn get_timeouts(&self) -> &ClientTimeouts {
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
            r.headers(headers)
                .optional_body(body)
                .trailers(trailers)
        })
    }

    async fn send_request_inner(
        &self,
        connection: &mut H2Connection,
        request: &Request,
    ) -> Result<u32, ProtocolError> {
        let stream_id = connection.create_stream().await?;

        let pseudo_headers = prepare_pseudo_headers(request)?;
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
            let mut connection = H2Connection::connect(request.target.url.as_str(), &timeouts).await?;
            let stream_id = self.send_request_inner(&mut connection, &request).await?;
            let response = connection.read_response(stream_id).await?;

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
