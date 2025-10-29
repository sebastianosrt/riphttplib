use super::error::ProtocolError;
use super::{Request, Response};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HttpProtocol {
    Http1,
    Http2,
    H2C,
    Http3,
}

#[async_trait(?Send)]
pub trait Protocol {
    async fn response(&self, request: Request) -> Result<Response, ProtocolError>;

    async fn send(&self, request: Request) -> Result<Response, ProtocolError> {
        self.response(request).await
    }
}
