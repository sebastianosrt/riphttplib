use super::Protocol;
use crate::types::{ClientTimeouts, ProtocolError, RequestBuilder, Response};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct ClientRequest<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    client: Option<C>,
    builder: RequestBuilder,
    future: Option<Pin<Box<dyn Future<Output = Result<Response, ProtocolError>>>>>,
}

impl<C> ClientRequest<C>
where
    C: Protocol + Send + Unpin + 'static,
{
    pub(super) fn new(client: C, method: &str, url: &str) -> Self {
        let method_upper = method.to_ascii_uppercase();
        Self {
            client: Some(client),
            builder: RequestBuilder::new(url, method_upper),
            future: None,
        }
    }

    pub fn header(mut self, header: impl AsRef<str>) -> Self {
        self.builder.header(header);
        self
    }

    pub fn headers<I, S>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.builder.headers(headers);
        self
    }

    pub fn data(mut self, body: impl AsRef<str>) -> Self {
        self.builder.data(body);
        self
    }

    pub fn body(mut self, body: impl AsRef<[u8]>) -> Self {
        self.builder.body(body);
        self
    }

    pub fn json(mut self, value: Value) -> Self {
        self.builder.json(value);
        self
    }

    pub fn params<I, K, V>(mut self, params: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.builder.params(params);
        self
    }

    pub fn trailer(mut self, trailer: impl AsRef<str>) -> Self {
        self.builder.trailer(trailer);
        self
    }

    pub fn trailers<I, S>(mut self, trailers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.builder.trailers(trailers);
        self
    }

    pub fn cookies<I, K, V>(mut self, cookies: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.builder.cookies(cookies);
        self
    }

    pub fn allow_redirects(mut self, allow: bool) -> Self {
        self.builder.allow_redirects(allow);
        self
    }

    pub fn timeout(mut self, timeout: ClientTimeouts) -> Self {
        self.builder.timeout(timeout);
        self
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

        let request = match this.builder.take() {
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
