use async_trait::async_trait;
use super::{Request, Response};
use super::error::ProtocolError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HttpProtocol {
    Http1,
    Http2,
    Http3,
}

#[async_trait(?Send)]
pub trait Protocol {
    async fn send(&self, request: Request) -> Result<Response, ProtocolError>;
}

