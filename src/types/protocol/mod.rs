use super::error::ProtocolError;
use super::{Request, Response};
use crate::utils::apply_redirect;
use async_trait::async_trait;

mod client;
mod client_request;

use bytes::Bytes;
pub use client::{Client, DefaultClient, TypedClient};
pub use client_request::ClientRequest;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HttpProtocol {
    Http1,
    Http2,
    H2C,
    Http3,
}

impl std::fmt::Display for HttpProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            HttpProtocol::Http1 => "HTTP/1.1",
            HttpProtocol::Http2 => "HTTP/2",
            HttpProtocol::H2C => "HTTP/2 (h2c)",
            HttpProtocol::Http3 => "HTTP/3",
        };
        write!(f, "{}", label)
    }
}

#[async_trait(?Send)]
pub trait Protocol {
    async fn execute(&self, request: &Request) -> Result<Response, ProtocolError>;

    async fn response(&self, mut request: Request) -> Result<Response, ProtocolError> {
        const MAX_REDIRECTS: u32 = 30;
        let mut redirect_count = 0u32;

        loop {
            let response = self.execute(&request).await?;

            if apply_redirect(&mut request, &response)? {
                redirect_count += 1;

                if redirect_count > MAX_REDIRECTS {
                    return Err(ProtocolError::RequestFailed(
                        "Too many redirects".to_string(),
                    ));
                }

                continue;
            }

            return Ok(response);
        }
    }

    async fn send_request(&self, request: Request) -> Result<Response, ProtocolError> {
        self.response(request).await
    }

    async fn send_raw(&self, _target: &str, _request: Bytes) -> Result<Response, ProtocolError> {
        Err(ProtocolError::RequestFailed(
            "Raw requests are not supported for this protocol".to_string(),
        ))
    }
}
