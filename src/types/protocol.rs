use super::error::ProtocolError;
use super::timeouts::ClientTimeouts;
use super::{Request, Response};
use crate::utils::parse_header;
use crate::{H1Client, H2Client, H3Client};
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

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

pub struct Client<C = H1Client>
where
    C: Protocol + DefaultClient + Send + Unpin + 'static,
{
    _marker: std::marker::PhantomData<C>,
}

pub trait DefaultClient: Protocol + Send + Unpin + 'static {
    fn default_client() -> Self;
}

impl DefaultClient for H1Client {
    fn default_client() -> Self {
        H1Client::new()
    }
}

impl DefaultClient for H2Client {
    fn default_client() -> Self {
        H2Client::new()
    }
}

impl DefaultClient for H3Client {
    fn default_client() -> Self {
        H3Client::new()
    }
}

impl<C> Client<C>
where
    C: Protocol + DefaultClient + Send + Unpin + 'static,
{
    pub fn request(method: &str, url: &str) -> ClientRequest<C> {
        ClientRequest::new(C::default_client(), method, url)
    }

    pub fn with(client: C) -> TypedClient<C> {
        TypedClient { client }
    }

    pub fn get(url: &str) -> ClientRequest<C> {
        Self::request("GET", url)
    }

    pub fn head(url: &str) -> ClientRequest<C> {
        Self::request("HEAD", url)
    }

    pub fn post(url: &str) -> ClientRequest<C> {
        Self::request("POST", url)
    }

    pub fn put(url: &str) -> ClientRequest<C> {
        Self::request("PUT", url)
    }

    pub fn patch(url: &str) -> ClientRequest<C> {
        Self::request("PATCH", url)
    }

    pub fn delete(url: &str) -> ClientRequest<C> {
        Self::request("DELETE", url)
    }

    pub fn options(url: &str) -> ClientRequest<C> {
        Self::request("OPTIONS", url)
    }

    pub fn trace(url: &str) -> ClientRequest<C> {
        Self::request("TRACE", url)
    }

    pub fn connect(url: &str) -> ClientRequest<C> {
        Self::request("CONNECT", url)
    }
}

pub struct TypedClient<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    client: C,
}

impl<C> TypedClient<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    pub fn request(&self, method: &str, url: &str) -> ClientRequest<C>
    where
        C: Clone,
    {
        ClientRequest::new(self.client.clone(), method, url)
    }

    pub fn get(&self, url: &str) -> ClientRequest<C>
    where
        C: Clone,
    {
        self.request("GET", url)
    }

    pub fn post(&self, url: &str) -> ClientRequest<C>
    where
        C: Clone,
    {
        self.request("POST", url)
    }
}

pub struct ClientRequest<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    client: Option<C>,
    request: Result<Request, ProtocolError>,
    future: Option<Pin<Box<dyn Future<Output = Result<Response, ProtocolError>>>>>,
}

impl<C> ClientRequest<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    fn new(client: C, method: &str, url: &str) -> Self {
        let method_upper = method.to_ascii_uppercase();
        Self {
            client: Some(client),
            request: Request::new(url, method_upper),
            future: None,
        }
    }

    fn map_request<F>(mut self, f: F) -> Self
    where
        F: FnOnce(Request) -> Result<Request, ProtocolError>,
    {
        self.request = self.request.and_then(f);
        self
    }

    pub fn header(self, header: impl AsRef<str>) -> Self {
        let header_text = header.as_ref().trim().to_string();
        self.map_request(move |req| {
            parse_header(&header_text)
                .map(|parsed| req.header(parsed))
                .ok_or_else(|| {
                    ProtocolError::MalformedHeaders(format!(
                        "Invalid header '{}'",
                        header_text
                    ))
                })
        })
    }

    pub fn headers<I, S>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.request = self.request.and_then(|mut req| {
            for header in headers.into_iter() {
                let header_text = header.as_ref().trim().to_string();
                let parsed = parse_header(&header_text).ok_or_else(|| {
                    ProtocolError::MalformedHeaders(format!(
                        "Invalid header '{}'",
                        header_text
                    ))
                })?;
                req = req.header(parsed);
            }
            Ok(req)
        });
        self
    }

    pub fn data(self, body: impl AsRef<str>) -> Self {
        let body_text = body.as_ref().to_string();
        self.map_request(|req| Ok(req.data(&body_text)))
    }

    pub fn body(self, body: impl AsRef<[u8]>) -> Self {
        let bytes = Bytes::copy_from_slice(body.as_ref());
        self.map_request(|req| Ok(req.body(bytes)))
    }

    pub fn json(self, value: Value) -> Self {
        self.map_request(|req| Ok(req.json(value)))
    }

    pub fn params<I, K, V>(self, params: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.map_request(|req| {
            let collected: Vec<(String, String)> =
                params.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
            Ok(req.params(collected))
        })
    }

    pub fn trailer(self, trailer: impl AsRef<str>) -> Self {
        let trailer_text = trailer.as_ref().trim().to_string();
        self.map_request(move |req| {
            parse_header(&trailer_text)
                .map(|parsed| req.trailer(parsed))
                .ok_or_else(|| {
                    ProtocolError::MalformedHeaders(format!(
                        "Invalid trailer '{}'",
                        trailer_text
                    ))
                })
        })
    }

    pub fn trailers<I, S>(mut self, trailers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.request = self.request.and_then(|mut req| {
            for trailer in trailers.into_iter() {
                let trailer_text = trailer.as_ref().trim().to_string();
                let parsed = parse_header(&trailer_text).ok_or_else(|| {
                    ProtocolError::MalformedHeaders(format!(
                        "Invalid trailer '{}'",
                        trailer_text
                    ))
                })?;
                req = req.trailer(parsed);
            }
            Ok(req)
        });
        self
    }

    pub fn cookies<I, K, V>(self, cookies: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.map_request(|req| {
            let collected: Vec<(String, String)> =
                cookies.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
            Ok(req.cookies(collected))
        })
    }

    pub fn allow_redirects(self, allow: bool) -> Self {
        self.map_request(|req| Ok(req.follow_redirects(allow)))
    }

    pub fn timeout(self, timeout: ClientTimeouts) -> Self {
        self.map_request(|req| Ok(req.timeout(timeout)))
    }
}

impl<C> Future for ClientRequest<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    type Output = Result<Response, ProtocolError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();

        if let Some(fut) = this.future.as_mut() {
            return fut.as_mut().poll(cx);
        }

        let request = match std::mem::replace(
            &mut this.request,
            Err(ProtocolError::RequestFailed(
                "request already consumed".to_string(),
            )),
        ) {
            Ok(req) => req,
            Err(err) => return Poll::Ready(Err(err)),
        };

        let client = match this.client.take() {
            Some(client) => client,
            None => {
                return Poll::Ready(Err(ProtocolError::RequestFailed(
                    "client already consumed".to_string(),
                )))
            }
        };

        let fut = async move { client.send_request(request).await };
        this.future = Some(Box::pin(fut));
        this.future
            .as_mut()
            .expect("future just set")
            .as_mut()
            .poll(cx)
    }
}
