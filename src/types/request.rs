use super::error::ProtocolError;
use super::timeouts::ClientTimeouts;
use super::{Header, Target};
use crate::parse_header;
use crate::types::proxy::ProxySettings;
use crate::utils::{
    ensure_user_agent, parse_headers, parse_target, parse_trailers, APPLICATION_JSON,
    CONTENT_TYPE_HEADER, COOKIE_HEADER,
};
use bytes::Bytes;
use serde_json::Value;
use url::form_urlencoded;

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

impl PreparedRequest {
    pub fn normalized_headers(&self) -> Vec<Header> {
        let mut headers = self.headers.clone();
        for header in &mut headers {
            header.normalize();
        }
        headers
    }

    pub fn header_block(&self) -> Vec<Header> {
        let mut block = self.pseudo_headers.clone();
        block.extend(self.normalized_headers());
        block
    }
}

#[derive(Debug)]
pub struct RequestBuilder {
    inner: Result<Request, ProtocolError>,
}

enum HeaderKind {
    Header,
    Trailer,
}

impl HeaderKind {
    fn description(&self) -> &'static str {
        match self {
            HeaderKind::Header => "header",
            HeaderKind::Trailer => "trailer",
        }
    }
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

    fn push_parsed_header(&mut self, kind: HeaderKind, header: Header) {
        if let Ok(request) = self.inner.as_mut() {
            match kind {
                HeaderKind::Header => request.header_mut(header),
                HeaderKind::Trailer => request.trailer_mut(header),
            }
        }
    }

    fn push_header_text(&mut self, kind: HeaderKind, text: &str) {
        if self.inner.is_err() {
            return;
        }

        let header_text = text.trim();
        let parsed = parse_header(header_text).ok_or_else(|| {
            ProtocolError::MalformedHeaders(format!(
                "Invalid {} '{}'",
                kind.description(),
                header_text
            ))
        });

        match parsed {
            Ok(header) => self.push_parsed_header(kind, header),
            Err(err) => self.inner = Err(err),
        }
    }
}

pub trait RequestBuilderOps {
    fn builder_mut(&mut self) -> &mut RequestBuilder;

    fn header(&mut self, header: &str) -> &mut Self {
        self.builder_mut()
            .push_header_text(HeaderKind::Header, header);
        self
    }

    fn header_value(&mut self, header: Header) -> &mut Self {
        self.builder_mut()
            .push_parsed_header(HeaderKind::Header, header);
        self
    }

    fn headers(&mut self, headers: Vec<String>) -> &mut Self {
        let builder = self.builder_mut();
        for header in headers {
            if builder.inner.is_err() {
                break;
            }
            builder.push_header_text(HeaderKind::Header, &header);
        }
        self
    }

    fn header_values(&mut self, headers: Vec<Header>) -> &mut Self {
        let builder = self.builder_mut();
        for header in headers {
            builder.push_parsed_header(HeaderKind::Header, header);
        }
        self
    }

    fn trailer(&mut self, trailer: &str) -> &mut Self {
        self.builder_mut()
            .push_header_text(HeaderKind::Trailer, trailer);
        self
    }

    fn trailers(&mut self, trailers: Vec<String>) -> &mut Self {
        let builder = self.builder_mut();
        for trailer in trailers {
            if builder.inner.is_err() {
                break;
            }
            builder.push_header_text(HeaderKind::Trailer, &trailer);
        }
        self
    }

    fn trailer_value(&mut self, trailer: Header) -> &mut Self {
        self.builder_mut()
            .push_parsed_header(HeaderKind::Trailer, trailer);
        self
    }

    fn trailer_values(&mut self, trailers: Vec<Header>) -> &mut Self {
        let builder = self.builder_mut();
        for trailer in trailers {
            builder.push_parsed_header(HeaderKind::Trailer, trailer);
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

    fn data<I, K, V>(&mut self, data: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_data(data);
        }
        self
    }

    fn query<I, K, V>(&mut self, query: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.set_params(query);
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

    fn proxies(&mut self, proxies: ProxySettings) -> &mut Self {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.proxies = Some(proxies);
        }
        self
    }

    fn proxy(&mut self, proxy_url: &str) -> &mut Self {
        let builder = self.builder_mut();
        if builder.inner.is_err() {
            return self;
        }

        let result = {
            let request = builder
                .inner
                .as_mut()
                .expect("checked inner state before proxy update");
            request.set_proxy(proxy_url)
        };

        if let Err(err) = result {
            builder.inner = Err(err);
        }
        self
    }

    fn without_proxies(&mut self) -> &mut Self {
        if let Ok(request) = self.builder_mut().inner.as_mut() {
            request.proxies = None;
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
    pub fn header(&mut self, header: &str) -> &mut Self {
        RequestBuilderOps::header(self, header)
    }

    pub fn headers(&mut self, headers: Vec<String>) -> &mut Self {
        RequestBuilderOps::headers(self, headers)
    }

    pub fn trailer(&mut self, trailer: &str) -> &mut Self {
        RequestBuilderOps::trailer(self, trailer)
    }

    pub fn trailers(&mut self, trailers: Vec<String>) -> &mut Self {
        RequestBuilderOps::trailers(self, trailers)
    }

    pub fn body(&mut self, body: impl AsRef<[u8]>) -> &mut Self {
        RequestBuilderOps::body(self, body)
    }

    pub fn json(&mut self, value: Value) -> &mut Self {
        RequestBuilderOps::json(self, value)
    }

    pub fn data<I, K, V>(&mut self, data: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        RequestBuilderOps::data(self, data)
    }

    pub fn query<I, K, V>(&mut self, query: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        RequestBuilderOps::query(self, query)
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

    pub fn proxies(&mut self, proxies: ProxySettings) -> &mut Self {
        RequestBuilderOps::proxies(self, proxies)
    }

    pub fn proxy(&mut self, proxy_url: &str) -> &mut Self {
        RequestBuilderOps::proxy(self, proxy_url)
    }

    pub fn without_proxies(&mut self) -> &mut Self {
        RequestBuilderOps::without_proxies(self)
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub target: Target,
    pub method: String,
    pub query: Vec<(String, String)>,
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
            query: Vec::new(),
            headers: Vec::new(),
            cookies: Vec::new(),
            trailers: Vec::new(),
            body: None,
            json: None,
            data: None,
            timeout: None,
            follow_redirects: true,
            proxies: None,
        })
    }

    pub fn builder(target: &str, method: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(target, method)
    }

    pub fn header(self, header: &str) -> Self {
        self.try_header(header)
            .unwrap_or_else(|_| panic!("Invalid header '{}': failed to parse", header.trim()))
    }

    pub fn try_header(mut self, header: &str) -> Result<Self, ProtocolError> {
        let parsed = parse_header(header.trim()).ok_or_else(|| {
            ProtocolError::MalformedHeaders(format!("Invalid header '{}'", header.trim()))
        })?;
        self.header_mut(parsed);
        Ok(self)
    }

    pub fn headers(self, headers: Vec<String>) -> Self {
        self.try_headers(headers)
            .unwrap_or_else(|err| panic!("Failed to parse headers: {:?}", err))
    }

    pub fn try_headers(mut self, headers: Vec<String>) -> Result<Self, ProtocolError> {
        let parsed = parse_headers(headers)?;
        self.headers_mut(parsed);
        Ok(self)
    }

    pub fn headers_from(mut self, headers: Vec<Header>) -> Self {
        self.headers_mut(headers);
        self
    }

    pub fn trailer(self, header: &str) -> Self {
        self.try_trailer(header)
            .unwrap_or_else(|_| panic!("Invalid trailer '{}': failed to parse", header.trim()))
    }

    pub fn try_trailer(mut self, header: &str) -> Result<Self, ProtocolError> {
        let parsed = parse_header(header.trim()).ok_or_else(|| {
            ProtocolError::MalformedHeaders(format!("Invalid trailer '{}'", header.trim()))
        })?;
        self.trailer_mut(parsed);
        Ok(self)
    }

    pub fn trailers(self, headers: Vec<String>) -> Self {
        self.try_trailers(headers)
            .unwrap_or_else(|err| panic!("Failed to parse trailers: {:?}", err))
    }

    pub fn try_trailers(mut self, headers: Vec<String>) -> Result<Self, ProtocolError> {
        let parsed = parse_trailers(headers)?;
        self.trailers_mut(parsed);
        Ok(self)
    }

    pub fn trailers_from(mut self, headers: Vec<Header>) -> Self {
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

    pub fn data<I, K, V>(mut self, data: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.set_data(data);
        self
    }

    pub fn query<I, K, V>(mut self, query: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.set_params(query);
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

    pub fn set_port(mut self, port: u16) -> Self {
        self.target.set_port(port);
        self
    }

    //

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

    pub fn set_data<I, K, V>(&mut self, data: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let fields: Vec<(String, String)> = data
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();

        let form_body = FormBody::Fields(fields);
        let encoded = form_body.encode();

        self.body = Some(Bytes::from(encoded.into_bytes()));
        self.data = Some(form_body);
        self.json = None;
    }

    pub fn set_params<I, K, V>(&mut self, query: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.query = query
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

        if self.query.is_empty() {
            // Fast path when no query
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
            // TODO write better
            // Estimate capacity to reduce allocations
            let estimated_param_size: usize = self
                .query
                .iter()
                .map(|(k, v)| k.len() + v.len() + 3) // +3 for =, &, and URL encoding overhead
                .sum();

            let total_capacity = path.len()
                + existing_query.map(|q| q.len() + 1).unwrap_or(0)
                + estimated_param_size
                + 10; // +10 buffer for URL encoding

            let mut serializer =
                form_urlencoded::Serializer::new(String::with_capacity(estimated_param_size));
            for (key, value) in &self.query {
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

    pub fn prepare_headers(&self) -> Vec<Header> {
        let mut headers: Vec<Header> = self
            .headers
            .iter()
            .filter(|h| !h.name.starts_with(':'))
            .cloned()
            .collect();

        if let Some(cookie_value) = self.cookie_header_value() {
            if !Self::has_header(&headers, COOKIE_HEADER) {
                headers.push(Header::new(COOKIE_HEADER.to_string(), cookie_value));
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
        // only :method is checked, need also authority, path ....

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
                // TOOD ??? remove path_only ???
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
                        request.path().to_string(), // path with query query
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

        Ok(pseudo_headers)
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

    pub fn proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, ProtocolError> {
        self.set_proxy(proxy_url)?;
        Ok(self)
    }

    pub fn set_proxy<S: AsRef<str>>(&mut self, proxy_url: S) -> Result<(), ProtocolError> {
        let inserted = self.proxies.is_none();
        let settings = self.proxies.get_or_insert_with(ProxySettings::default);
        if let Err(err) = settings.set_proxy(proxy_url.as_ref()) {
            if inserted {
                self.proxies = None;
            }
            return Err(err);
        }
        Ok(())
    }

    pub fn without_proxies(mut self) -> Self {
        self.proxies = None;
        self
    }
}
