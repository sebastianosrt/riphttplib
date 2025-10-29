use bytes::Bytes;
use serde_json::Value;
use url::{form_urlencoded, Url};

use super::error::ProtocolError;
use super::timeouts::ClientTimeouts;
use super::{Header, Target};
use crate::types::proxy::{ProxyConfig, ProxySettings};
use crate::utils::parse_target;

#[derive(Debug, Clone)]
pub struct Request {
    pub target: Target,
    pub method: String,
    pub headers: Vec<Header>,
    pub body: Option<Bytes>,
    pub trailers: Option<Vec<Header>>,
    pub params: Vec<(String, String)>,
    pub json: Option<Value>,
    pub cookies: Vec<(String, String)>,
    pub timeout: Option<ClientTimeouts>,
    pub allow_redirects: bool,
    pub proxies: Option<ProxySettings>,
    pub is_chunked: Option<bool>,
}

impl Request {
    pub fn new(target: &str, method: impl Into<String>) -> Result<Self, ProtocolError> {
        Ok(Self {
            target: parse_target(target)?,
            method: method.into(),
            headers: Vec::new(),
            body: None,
            trailers: None,
            params: Vec::new(),
            json: None,
            cookies: Vec::new(),
            timeout: None,
            allow_redirects: true,
            proxies: None,
            is_chunked: None,
        })
    }

    pub fn header(mut self, header: Header) -> Self {
        self.headers.push(header);
        self
    }

    pub fn headers(mut self, headers: Vec<Header>) -> Self {
        self.headers = headers;
        self
    }

    pub fn body<B: Into<Bytes>>(mut self, body: B) -> Self {
        self.body = Some(body.into());
        self.json = None;
        self
    }

    pub fn optional_body<B: Into<Bytes>>(mut self, body: Option<B>) -> Self {
        self.body = body.map(Into::into);
        if self.body.is_some() {
            self.json = None;
        }
        self
    }

    pub fn trailers(mut self, trailers: Option<Vec<Header>>) -> Self {
        self.trailers = trailers;
        self
    }

    pub fn params<I, K, V>(mut self, params: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.params = params
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self
    }

    pub fn json(mut self, json: Value) -> Self {
        let serialized =
            serde_json::to_vec(&json).expect("serializing JSON body into bytes must succeed");
        self.body = Some(Bytes::from(serialized));
        self.json = Some(json);
        self
    }

    pub fn cookies<I, K, V>(mut self, cookies: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.cookies = cookies
            .into_iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect();
        self
    }

    pub fn timeout(mut self, timeouts: ClientTimeouts) -> Self {
        self.timeout = Some(timeouts);
        self
    }

    pub fn allow_redirects(mut self, allow: bool) -> Self {
        self.allow_redirects = allow;
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

    pub fn effective_headers(&self) -> Vec<Header> {
        // Pre-calculate capacity to avoid reallocations
        let additional_headers = (if self.json.is_some()
            && !Self::has_header_case_insensitive(&self.headers, "content-type")
        {
            1
        } else {
            0
        }) + (if !self.cookies.is_empty()
            && !Self::has_header_case_insensitive(&self.headers, "cookie")
        {
            1
        } else {
            0
        });

        let mut headers = Vec::with_capacity(self.headers.len() + additional_headers);
        headers.extend_from_slice(&self.headers);

        if self.json.is_some()
            && !Self::has_header_case_insensitive(&headers, crate::utils::CONTENT_TYPE_HEADER)
        {
            headers.push(Header::new(
                crate::utils::CONTENT_TYPE_HEADER.to_string(),
                crate::utils::APPLICATION_JSON.to_string(),
            ));
        }

        if let Some(cookie_value) = self.cookie_header_value() {
            if !Self::has_header_case_insensitive(&headers, crate::utils::COOKIE_HEADER) {
                headers.push(Header::new(
                    crate::utils::COOKIE_HEADER.to_string(),
                    cookie_value,
                ));
            }
        }

        headers
    }

    pub fn effective_timeouts(&self, fallback: &ClientTimeouts) -> ClientTimeouts {
        self.timeout.clone().unwrap_or_else(|| fallback.clone())
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

    fn has_header_case_insensitive(headers: &[Header], name: &str) -> bool {
        headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case(name))
    }
}
