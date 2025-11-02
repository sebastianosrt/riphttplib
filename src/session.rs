use crate::h1::protocol::H1;
use crate::h2::protocol::H2;
use crate::h3::protocol::H3;
use crate::types::{
    ClientTimeouts, Header, Protocol, ProtocolError, ProxySettings, Request, RequestBuilder,
    RequestBuilderOps, Response,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Default)]
pub struct CookieStore {
    entries: BTreeMap<String, String>,
}

impl CookieStore {
    fn apply_to_request(&self, request: &mut Request) {
        if self.entries.is_empty() {
            return;
        }

        let mut combined: BTreeMap<String, String> = self.entries.clone();
        for (name, value) in &request.cookies {
            combined.insert(name.clone(), value.clone());
        }

        request.cookies = combined.into_iter().collect();
    }

    fn update_from_response(&mut self, response: &Response) {
        for (name, value) in &response.cookies {
            if value.is_empty() {
                self.entries.remove(name);
            } else {
                self.entries.insert(name.clone(), value.clone());
            }
        }
    }

    pub fn set_cookie(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(name.into(), value.into());
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.entries.iter()
    }
}

impl fmt::Display for CookieStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (name, value) in &self.entries {
            if !first {
                write!(f, "; ")?;
            }
            write!(f, "{}={}", name, value)?;
            first = false;
        }
        if first {
            write!(f, "<empty cookies>")?;
        }
        Ok(())
    }
}

fn apply_default_headers(defaults: &[Header], request: &mut Request) {
    for header in defaults {
        let exists = request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(&header.name));
        if !exists {
            request.headers.push(header.clone());
        }
    }
}

pub struct Session<P>
where
    P: Protocol + Clone,
{
    client: P,
    default_headers: Vec<Header>,
    pub cookies: CookieStore,
}

impl<P> Session<P>
where
    P: Protocol + Clone,
{
    pub(crate) fn new(client: P) -> Self {
        Self {
            client,
            default_headers: Vec::new(),
            cookies: CookieStore::default(),
        }
    }

    pub fn add_default_header(&mut self, header: Header) {
        if !self
            .default_headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(&header.name))
        {
            self.default_headers.push(header);
        }
    }

    pub fn header(&mut self, header: Header) {
        self.add_default_header(header);
    }

    pub fn set_cookie(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.cookies.set_cookie(name, value);
    }

    pub fn request<'a>(&'a mut self, method: &str, url: &str) -> SessionRequestBuilder<'a, P> {
        SessionRequestBuilder::new(self, method, url)
    }

    pub fn get(&mut self, url: &str) -> SessionRequestBuilder<'_, P> {
        self.request("GET", url)
    }

    pub fn head(&mut self, url: &str) -> SessionRequestBuilder<'_, P> {
        self.request("HEAD", url)
    }

    pub fn post(&mut self, url: &str) -> SessionRequestBuilder<'_, P> {
        self.request("POST", url)
    }

    pub fn put(&mut self, url: &str) -> SessionRequestBuilder<'_, P> {
        self.request("PUT", url)
    }

    pub fn patch(&mut self, url: &str) -> SessionRequestBuilder<'_, P> {
        self.request("PATCH", url)
    }

    pub fn delete(&mut self, url: &str) -> SessionRequestBuilder<'_, P> {
        self.request("DELETE", url)
    }

    pub fn options(&mut self, url: &str) -> SessionRequestBuilder<'_, P> {
        self.request("OPTIONS", url)
    }

    pub async fn send(&mut self, mut request: Request) -> Result<Response, ProtocolError> {
        self.prepare_request(&mut request);
        let response = self.client.send_request(request).await?;
        self.finalize_response(&response);
        Ok(response)
    }

    fn prepare_request(&self, request: &mut Request) {
        apply_default_headers(&self.default_headers, request);
        self.cookies.apply_to_request(request);
    }

    fn finalize_response(&mut self, response: &Response) {
        self.cookies.update_from_response(response);
    }

    pub fn client(&self) -> &P {
        &self.client
    }
}

pub type H1Session = Session<H1>;
pub type H2Session = Session<H2>;
pub type H3Session = Session<H3>;

pub struct SessionRequestBuilder<'a, P>
where
    P: Protocol + Clone,
{
    session: &'a mut Session<P>,
    builder: RequestBuilder,
}

impl<'a, P> SessionRequestBuilder<'a, P>
where
    P: Protocol + Clone,
{
    fn new(session: &'a mut Session<P>, method: &str, url: &str) -> Self {
        let method = method.to_ascii_uppercase();
        Self {
            session,
            builder: RequestBuilder::new(url, method),
        }
    }

    pub fn header(mut self, header: &str) -> Self {
        RequestBuilderOps::header(&mut self, header);
        self
    }

    pub fn header_value(mut self, header: Header) -> Self {
        RequestBuilderOps::header_value(&mut self, header);
        self
    }

    pub fn headers(mut self, headers: Vec<String>) -> Self {
        RequestBuilderOps::headers(&mut self, headers);
        self
    }

    pub fn header_values(mut self, headers: Vec<Header>) -> Self {
        RequestBuilderOps::header_values(&mut self, headers);
        self
    }

    pub fn trailer(mut self, trailer: &str) -> Self {
        RequestBuilderOps::trailer(&mut self, trailer);
        self
    }

    pub fn trailer_value(mut self, trailer: Header) -> Self {
        RequestBuilderOps::trailer_value(&mut self, trailer);
        self
    }

    pub fn trailers(mut self, trailers: Vec<String>) -> Self {
        RequestBuilderOps::trailers(&mut self, trailers);
        self
    }

    pub fn trailer_values(mut self, trailers: Vec<Header>) -> Self {
        RequestBuilderOps::trailer_values(&mut self, trailers);
        self
    }

    pub fn body<B>(mut self, body: B) -> Self
    where
        B: AsRef<[u8]>,
    {
        RequestBuilderOps::body(&mut self, body);
        self
    }

    pub fn json(mut self, value: Value) -> Self {
        RequestBuilderOps::json(&mut self, value);
        self
    }

    pub fn data<I, K, V>(mut self, data: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        RequestBuilderOps::data(&mut self, data);
        self
    }

    pub fn query<I, K, V>(mut self, query: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        RequestBuilderOps::query(&mut self, query);
        self
    }

    pub fn cookies<I, K, V>(mut self, cookies: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        RequestBuilderOps::cookies(&mut self, cookies);
        self
    }

    pub fn allow_redirects(mut self, allow: bool) -> Self {
        RequestBuilderOps::allow_redirects(&mut self, allow);
        self
    }

    pub fn follow_redirects(self, allow: bool) -> Self {
        self.allow_redirects(allow)
    }

    pub fn timeout(mut self, timeout: ClientTimeouts) -> Self {
        RequestBuilderOps::timeout(&mut self, timeout);
        self
    }

    pub fn proxies(mut self, proxies: ProxySettings) -> Self {
        RequestBuilderOps::proxies(&mut self, proxies);
        self
    }

    pub fn proxy(mut self, proxy_url: &str) -> Self {
        RequestBuilderOps::proxy(&mut self, proxy_url);
        self
    }

    pub fn without_proxies(mut self) -> Self {
        RequestBuilderOps::without_proxies(&mut self);
        self
    }

    pub async fn send(self) -> Result<Response, ProtocolError> {
        let SessionRequestBuilder { session, builder } = self;
        let request = builder.build()?;
        session.send(request).await
    }
}

impl<'a, P> RequestBuilderOps for SessionRequestBuilder<'a, P>
where
    P: Protocol + Clone,
{
    fn builder_mut(&mut self) -> &mut RequestBuilder {
        &mut self.builder
    }
}
