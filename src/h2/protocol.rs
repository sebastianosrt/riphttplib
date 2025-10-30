use crate::h2::connection::H2Connection;
use crate::types::{
    ClientTimeouts, H2StreamErrorKind, Header, Protocol, ProtocolError, Request, Response,
};
use crate::utils::{
    apply_redirect, ensure_user_agent, merge_headers, prepare_pseudo_headers,
};
use async_trait::async_trait;
use bytes::Bytes;

#[derive(Clone)]
pub struct H2 {
    timeouts: ClientTimeouts,
}

impl H2 {
    pub fn new() -> Self {
        Self::timeouts(ClientTimeouts::default())
    }

    pub fn timeouts(timeouts: ClientTimeouts) -> Self {
        Self { timeouts }
    }

    pub fn get_timeouts(&self) -> &ClientTimeouts {
        &self.timeouts
    }

    pub fn session(&self) -> crate::session::H2Session {
        crate::session::H2Session::new(self.clone())
    }

    pub fn build_request(
        target: &str,
        method: impl Into<String>,
        headers: Vec<Header>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Request, ProtocolError> {
        crate::utils::build_request(target, method, headers, body, trailers)
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
        let has_trailers = !request.trailers.is_empty();


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

        if !request.trailers.is_empty() {
            // let normalized_trailers = normalize_headers(&request.trailers.clone()); //
            connection
                .send_headers(stream_id, &request.trailers, true)
                .await
                .map_err(|e| {
                    ProtocolError::H2StreamError(H2StreamErrorKind::ProtocolViolation(format!(
                        "Failed to send trailers: {}",
                        e
                    )))
                })?;
        }

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

            let timeouts = request.effective_timeouts(&self.timeouts);
            let mut connection =
                H2Connection::connect(request.target.url.as_str(), &timeouts).await?;
            let stream_id = self.send_request_inner(&mut connection, &request).await?;
            let response = connection.read_response(stream_id).await?;

            if apply_redirect(&mut request, &response)? {
                return self
                    .send_request_internal(request, redirect_count + 1)
                    .await;
            }

            Ok(response)
        })
    }
}

#[async_trait(?Send)]
impl Protocol for H2 {
    async fn response(&self, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(request).await
    }
}
