use super::error::ProtocolError;
use super::{Request, Response};
use async_trait::async_trait;

mod client;
mod client_request;

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
    async fn response(&self, request: Request) -> Result<Response, ProtocolError>;

    async fn send_request(&self, request: Request) -> Result<Response, ProtocolError> {
        self.response(request).await
    }

}
