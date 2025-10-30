use bytes::Bytes;
use serde_json::Value;
use url::{form_urlencoded, Url};
use super::error::ProtocolError;
use super::timeouts::ClientTimeouts;
use super::{Header, Target};
use crate::types::proxy::{ProxyConfig, ProxySettings};
use crate::utils::{
    ensure_user_agent, parse_target, APPLICATION_JSON, CONTENT_TYPE_HEADER, COOKIE_HEADER,
};
use crate::parse_header;

const APPLICATION_X_WWW_FORM_URLENCODED: &str = "application/x-www-form-urlencoded";

#[derive(Debug, Clone)]
pub enum FormBody {
    Raw(String),
    Fields(Vec<(String, String)>),
}

impl FormBody {
    fn encode(&self) -> String {
        match self {
            FormBody::Raw(value) => value.clone(),
            FormBody::Fields(pairs) => {
                let mut serializer = form_urlencoded::Serializer::new(String::new());
                for (key, value) in pairs {
                    serializer.append_pair(key, value);
                }
                serializer.finish()
            }
        }
    }
}

impl From<&str> for FormBody {
    fn from(value: &str) -> Self {
        FormBody::Raw(value.to_string())
    }
}

impl From<String> for FormBody {
    fn from(value: String) -> Self {
        FormBody::Raw(value)
    }
}

impl<K, V> From<Vec<(K, V)>> for FormBody
where
    K: Into<String>,
    V: Into<String>,
{
    fn from(pairs: Vec<(K, V)>) -> Self {
        FormBody::Fields(
            pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct PreparedRequest {
    pub method: String,
    pub path: String,
    pub pseudo_headers: Vec<Header>,
    pub headers: Vec<Header>,
    pub body: Option<Bytes>,
    pub trailers: Vec<Header>,
}

#[derive(Debug)]
pub struct RequestBuilder {
    inner: Result<Request, ProtocolError>,
}

impl RequestBuilder {
    pub fn new(target: &str, method: impl Into<String>) -> Self {
        let method = method.into();
        let inner = Request::new(target, method);
        Self { inner }
    }

    pub fn from_request(request: Request) -> Self {
        Self { inner: Ok(request) }
    }

    pub fn build(self) -> Result<Request, ProtocolError> {
        self.inner
    }

pub fn take(&mut self) -> Result<Request, ProtocolError> {
        std::mem::replace(
            &mut self.inner,
            Err(ProtocolError::RequestFailed(
                "request already consumed".to_string(),
            )),
        )
    }
}

pub trait RequestBuilderOps {
    fn builder_mut(&mut self) -> &mut RequestBuilder;

    fn header(&mut self, header: impl AsRef<str>) -> &mut Self {
        let builder = self.builder_mut();
        if builder.inner.is_err() {
            return self;
        }

        let header_text = header.as_ref().trim().to_string();
        let parsed = match parse_header(&header_text) {
            Some(parsed) => parsed,
            None => {
                builder.inner = Err(ProtocolError::MalformedHeaders(format!(
                    "Invalid header '{}'",
                    header_text
                )));
                return self;
            }
        };

        if let Ok(request) = builder.inner.as_mut() {
            request.header_mut(parsed);
        }

        self
    }

    fn headers<I, S>(&mut self, headers: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for header in headers {
            if self.builder_mut().inner.is_err() {
                break;
            }
            self.header(header);
        }
        self
    }

    fn trailer(&mut self, trailer: impl AsRef<str>) -> &mut Self {
        let builder = self.builder_mut();
        if builder.inner.is_err() {
            return self;
        }

        let trailer_text = trailer.as_ref().trim().to_string();
        let parsed = match parse_header(&trailer_text) {
            Some(parsed) => parsed,
            None => {
                builder.inner = Err(ProtocolError::MalformedHeaders(format!(
                    "Invalid trailer '{}'",
                    trailer_text
                )));
                return self;
            }
        };

        if let Ok(request) = builder.inner.as_mut() {
            request.trailer_mut(parsed);
        }

        self
    }

    fn trailers<I, S>(&mut self, trailers: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for trailer in trailers {
            if self.builder_mut().inner.is_err() {
                break;
            }
            self.trailer(trailer);
        }
        self
    }

    fn body(&mut self, body: impl AsRef<[u8]>) -> &mut Self {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_body(Bytes::copy_from_slice(body.as_ref()));
        }
        self
    }

    fn json(&mut self, value: Value) -> &mut Self {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_json(value);
        }
        self
    }

    fn data(&mut self, data: impl AsRef<str>) -> &mut Self {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_data(data.as_ref());
        }
        self
    }

    fn params<I, K, V>(&mut self, params: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_params(params);
        }
        self
    }

    fn cookies<I, K, V>(&mut self, cookies: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_cookies(cookies);
        }
        self
    }

    fn allow_redirects(&mut self, allow: bool) -> &mut Self {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_follow_redirects(allow);
        }
        self
    }

    fn timeout(&mut self, timeout: ClientTimeouts) -> &mut Self {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_timeout(timeout);
        }
        self
    }
}

impl RequestBuilderOps for RequestBuilder {
    fn builder_mut(&mut self) -> &mut RequestBuilder {
        self
    }
}

impl RequestBuilder {
    pub fn header(&mut self, header: impl AsRef<str>) -> &mut Self {
        RequestBuilderOps::header(self, header)
    }

    pub fn headers<I, S>(&mut self, headers: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        RequestBuilderOps::headers(self, headers)
    }

    pub fn trailer(&mut self, trailer: impl AsRef<str>) -> &mut Self {
        RequestBuilderOps::trailer(self, trailer)
    }

    pub fn trailers<I, S>(&mut self, trailers: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        RequestBuilderOps::trailers(self, trailers)
    }

    pub fn body(&mut self, body: impl AsRef<[u8]>) -> &mut Self {
        RequestBuilderOps::body(self, body)
    }

    pub fn json(&mut self, value: Value) -> &mut Self {
        RequestBuilderOps::json(self, value)
    }

    pub fn data(&mut self, data: impl AsRef<str>) -> &mut Self {
        RequestBuilderOps::data(self, data)
    }

    pub fn params<I, K, V>(&mut self, params: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        RequestBuilderOps::params(self, params)
    }

    pub fn cookies<I, K, V>(&mut self, cookies: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        RequestBuilderOps::cookies(self, cookies)
    }

    pub fn allow_redirects(&mut self, allow: bool) -> &mut Self {
        RequestBuilderOps::allow_redirects(self, allow)
    }

    pub fn timeout(&mut self, timeout: ClientTimeouts) -> &mut Self {
        RequestBuilderOps::timeout(self, timeout)
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub target: Target,
    pub method: String,
    pub params: Vec<(String, String)>,
    pub headers: Vec<Header>,
    pub trailers: Vec<Header>,
    pub cookies: Vec<(String, String)>,
    pub body: Option<Bytes>,
    pub json: Option<Value>,
    pub data: Option<FormBody>,
    pub timeout: Option<ClientTimeouts>,
    pub follow_redirects: bool,
    pub proxies: Option<ProxySettings>,
}

impl Request {
    pub fn new(target: &str, method: impl Into<String>) -> Result<Self, ProtocolError> {
        Ok(Self {
            target: parse_target(target)?,
            method: method.into(),
            params: Vec::new(),
            headers: Vec::new(),
            cookies: Vec::new(),
            trailers: Vec::new(),
            body: None,
            json: None,
            data: None,
            timeout: None,
            follow_redirects: true,
            proxies: None
        })
    }

    pub fn builder(target: &str, method: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(target, method)
    }

    pub fn header(mut self, header: Header) -> Self {
        self.header_mut(header);
        self
    }

    pub fn headers(mut self, headers: Vec<Header>) -> Self {
        self.headers_mut(headers);
        self
    }

    pub fn trailer(mut self, header: Header) -> Self {
        self.trailer_mut(header);
        self
    }

    pub fn trailers(mut self, headers: Vec<Header>) -> Self {
        self.trailers_mut(headers);
        self
    }

    pub fn body<B: Into<Bytes>>(mut self, body: B) -> Self {
        self.set_body(body);
        self
    }

    pub fn json(mut self, json: Value) -> Self {
        self.set_json(json);
        self
    }

    pub fn data<T>(mut self, data: T) -> Self
    where
        T: Into<FormBody>,
    {
        self.set_data(data);
        self
    }

    pub fn params<I, K, V>(mut self, params: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.set_params(params);
        self
    }

    pub fn cookies<I, K, V>(mut self, cookies: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.set_cookies(cookies);
        self
    }

    pub fn header_mut(&mut self, header: Header) {
        self.headers.push(header);
    }

    pub fn headers_mut(&mut self, headers: Vec<Header>) {
        self.headers = headers;
    }

    pub fn trailer_mut(&mut self, header: Header) {
        self.trailers.push(header);
    }

    pub fn trailers_mut(&mut self, headers: Vec<Header>) {
        self.trailers = headers;
    }

    pub fn set_body<B: Into<Bytes>>(&mut self, body: B) {
        self.body = Some(body.into());
        self.json = None;
        self.data = None;
    }

    pub fn set_json(&mut self, json: Value) {
        let serialized =
            serde_json::to_vec(&json).expect("serializing JSON body into bytes must succeed");
        self.body = Some(Bytes::from(serialized));
        self.json = Some(json);
        self.data = None;
    }

    pub fn set_data<T>(&mut self, data: T)
    where
        T: Into<FormBody>,
    {
        let form_body: FormBody = data.into();
        let encoded = form_body.encode();
        self.body = Some(Bytes::from(encoded.into_bytes()));
        self.data = Some(form_body);
        self.json = None;
    }

    pub fn set_params<I, K, V>(&mut self, params: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.params = params
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
    }

    pub fn set_cookies<I, K, V>(&mut self, cookies: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.cookies = cookies
            .into_iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect();
    }

    pub fn set_timeout(&mut self, timeouts: ClientTimeouts) {
        self.timeout = Some(timeouts);
    }

    pub fn set_follow_redirects(&mut self, allow: bool) {
        self.follow_redirects = allow;
    }

    pub fn path(&self) -> String {
        let path = self.target.url.path();
        let path = if path.is_empty() { "/" } else { path };

        let existing_query = self.target.url.query();

        if self.params.is_empty() {
            // Fast path when no params
            match existing_query {
                Some(query) => {
                    let mut result = String::with_capacity(path.len() + query.len() + 1);
                    result.push_str(path);
                    result.push('?');
                    result.push_str(query);
                    result
                }
                None => path.to_string(),
            }
        } else {
            // Estimate capacity to reduce allocations
            let estimated_param_size: usize = self
                .params
                .iter()
                .map(|(k, v)| k.len() + v.len() + 3) // +3 for =, &, and URL encoding overhead
                .sum();

            let total_capacity = path.len()
                + existing_query.map(|q| q.len() + 1).unwrap_or(0)
                + estimated_param_size
                + 10; // +10 buffer for URL encoding

            let mut serializer =
                form_urlencoded::Serializer::new(String::with_capacity(estimated_param_size));
            for (key, value) in &self.params {
                serializer.append_pair(key, value);
            }
            let new_query = serializer.finish();

            let mut result = String::with_capacity(total_capacity);
            result.push_str(path);
            result.push('?');

            if let Some(existing) = existing_query {
                result.push_str(existing);
                if !new_query.is_empty() {
                    result.push('&');
                    result.push_str(&new_query);
                }
            } else {
                result.push_str(&new_query);
            }

            result
        }
    }

    fn prepare_headers(&self) -> Vec<Header> {
        let mut headers: Vec<Header> = self
            .headers
            .iter()
            .filter(|h| !h.name.starts_with(':'))
            .cloned()
            .collect();

        if let Some(cookie_value) = self.cookie_header_value() {
            if !Self::has_header(&headers, COOKIE_HEADER) {
                headers.push(Header::new(
                    COOKIE_HEADER.to_string(),
                    cookie_value,
                ));
            }
        }

        if !Self::has_header(&headers, CONTENT_TYPE_HEADER) {
            if self.json.is_some() {
                headers.push(Header::new(
                    CONTENT_TYPE_HEADER.to_string(),
                    APPLICATION_JSON.to_string(),
                ));
            } else if self.data.is_some() {
                headers.push(Header::new(
                    CONTENT_TYPE_HEADER.to_string(),
                    APPLICATION_X_WWW_FORM_URLENCODED.to_string(),
                ));
            }
        }

        ensure_user_agent(&mut headers);

        headers
    }

    pub fn effective_headers(&self) -> Vec<Header> {
        self.prepare_headers()
    }

    pub fn prepare_pseudo_headers(request: &Request) -> Result<Vec<Header>, ProtocolError> {
        let mut pseudo_headers: Vec<Header> = request
            .headers
            .iter()
            .filter(|h| h.name.starts_with(':'))
            .cloned()
            .collect();

        if !pseudo_headers.iter().any(|h| h.name == ":method") {
            pseudo_headers.insert(
                0,
                Header::new(":method".to_string(), request.method.clone()),
            );
        }

        let method = request.method.to_uppercase();
        // TODO check correctness
        match method.as_str() {
            "CONNECT" => {
                if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                    let authority = request.target.authority().ok_or_else(|| {
                        ProtocolError::InvalidTarget(
                            "CONNECT requests require an authority".to_string(),
                        )
                    })?;
                    pseudo_headers.push(Header::new(":authority".to_string(), authority));
                }
                pseudo_headers.retain(|h| h.name != ":scheme" && h.name != ":path");
            }
            "OPTIONS" => {
                let path_value = if request.target.path_only() == "*" {
                    "*".to_string()
                } else {
                    request.target.path().to_string()
                };
                if !pseudo_headers.iter().any(|h| h.name == ":path") {
                    pseudo_headers.push(Header::new(":path".to_string(), path_value));
                }
                if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                    if let Some(authority) = request.target.authority() {
                        pseudo_headers.push(Header::new(":authority".to_string(), authority));
                    }
                }
                if !pseudo_headers.iter().any(|h| h.name == ":scheme") {
                    pseudo_headers.push(Header::new(
                        ":scheme".to_string(),
                        request.target.scheme().to_string(),
                    ));
                }
            }
            _ => {
                if !pseudo_headers.iter().any(|h| h.name == ":path") {
                    pseudo_headers.push(Header::new(
                        ":path".to_string(),
                        request.path().to_string(), // path with query params
                    ));
                }
                if !pseudo_headers.iter().any(|h| h.name == ":scheme") {
                    pseudo_headers.push(Header::new(
                        ":scheme".to_string(),
                        request.target.scheme().to_string(),
                    ));
                }
                if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                    if let Some(authority) = request.target.authority() {
                        pseudo_headers.push(Header::new(":authority".to_string(), authority));
                    }
                }
            }
        }

        Ok(pseudo_headers) // TODO normalization should happen only in Header::new
    }

    fn cookie_header_value(&self) -> Option<String> {
        if self.cookies.is_empty() {
            None
        } else {
            // Pre-calculate capacity to avoid reallocations
            let estimated_size: usize = self
                .cookies
                .iter()
                .map(|(name, value)| name.len() + value.len() + 3) // +3 for "=", "; "
                .sum();

            let mut result = String::with_capacity(estimated_size);
            let mut first = true;

            for (name, value) in &self.cookies {
                if !first {
                    result.push_str("; ");
                }
                result.push_str(name);
                result.push('=');
                result.push_str(value);
                first = false;
            }

            Some(result)
        }
    }

    fn has_header(headers: &[Header], name: &str) -> bool {
        headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case(name))
    }


    ///

    /* prepare the real request to be sent, the request should have:
        - method
        - path
        - pseudoheaders if http2/3
        - headers
        - body
        - trailers
    */
    pub fn prepare_request(&self) -> Result<PreparedRequest, ProtocolError> {
        let headers = self.prepare_headers();
        let pseudo_headers = Self::prepare_pseudo_headers(self)?;

        Ok(PreparedRequest {
            method: self.method.clone(),
            path: self.path(),
            pseudo_headers,
            headers,
            body: self.body.clone(),
            trailers: self.trailers.clone(),
        })
    }

    ///


    pub fn timeouts(&self, fallback: &ClientTimeouts) -> ClientTimeouts {
        self.timeout.clone().unwrap_or_else(|| fallback.clone())
    }

    pub fn timeout(mut self, timeouts: ClientTimeouts) -> Self {
        self.set_timeout(timeouts);
        self
    }

    pub fn follow_redirects(mut self, allow: bool) -> Self {
        self.set_follow_redirects(allow);
        self
    }

    pub fn proxies(mut self, proxies: ProxySettings) -> Self {
        self.proxies = Some(proxies);
        self
    }

    pub fn http_proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.http = Some(Url::parse(proxy_url.as_ref())?);
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn https_proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.https = Some(Url::parse(proxy_url.as_ref())?);
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn socks5_proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.socks = Some(ProxyConfig::socks5(Url::parse(proxy_url.as_ref())?));
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn socks5_proxy_auth<S: AsRef<str>>(
        mut self,
        proxy_url: S,
        username: String,
        password: String,
    ) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.socks =
            Some(ProxyConfig::socks5(Url::parse(proxy_url.as_ref())?).auth(username, password));
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn socks4_proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.socks = Some(ProxyConfig::socks4(Url::parse(proxy_url.as_ref())?));
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn socks4_proxy_auth<S: AsRef<str>>(
        mut self,
        proxy_url: S,
        username: String,
    ) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.socks = Some(
            ProxyConfig::socks4(Url::parse(proxy_url.as_ref())?).auth(username, String::new()),
        );
        self.proxies = Some(proxies);
        Ok(self)
    }

pub fn without_proxies(mut self) -> Self {
        self.proxies = None;
        self
    }
}