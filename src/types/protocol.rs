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

    async fn get(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "GET")
    }

    async fn head(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "HEAD")
    }

    async fn post(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "POST")
    }

    async fn put(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "PUT")
    }

    async fn patch(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "PATCH")
    }

    async fn delete(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "DELETE")
    }

    async fn options(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "OPTIONS")
    }

    async fn trace(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "TRACE")
    }

    async fn connect(url: &str) -> Result<Request, ProtocolError> {
        Request::new(url, "CONNECT")
    }
}
