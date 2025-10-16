
use bytes::Bytes;
use serde_json::Value;
use std::time::Duration;
use url::{form_urlencoded, Url};

use crate::utils::parse_target;
use super::{Target, Header};
use super::error::ProtocolError;

#[derive(Debug, Clone, PartialEq)]
pub enum ProxyType {
    Http,
    Https,
    Socks5,
    Socks4,
}

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub url: Url,
    pub proxy_type: ProxyType,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ProxyConfig {
    pub fn new(url: Url, proxy_type: ProxyType) -> Self {
        Self {
            url,
            proxy_type,
            username: None,
            password: None,
        }
    }

    pub fn with_auth(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }

    pub fn http(url: Url) -> Self {
        Self::new(url, ProxyType::Http)
    }

    pub fn https(url: Url) -> Self {
        Self::new(url, ProxyType::Https)
    }

    pub fn socks5(url: Url) -> Self {
        Self::new(url, ProxyType::Socks5)
    }

    pub fn socks4(url: Url) -> Self {
        Self::new(url, ProxyType::Socks4)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProxySettings {
    pub http: Option<Url>,
    pub https: Option<Url>,
    pub socks: Option<ProxyConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct ProxySettingsBuilder {
    pub http: Option<String>,
    pub https: Option<String>,
    pub socks: Option<String>,
}

impl ProxySettingsBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn http<S: Into<String>>(mut self, url: S) -> Self {
        self.http = Some(url.into());
        self
    }

    pub fn https<S: Into<String>>(mut self, url: S) -> Self {
        self.https = Some(url.into());
        self
    }

    pub fn socks<S: Into<String>>(mut self, url: S) -> Self {
        self.socks = Some(url.into());
        self
    }

    pub fn build(self) -> Result<ProxySettings, url::ParseError> {
        Ok(ProxySettings {
            http: if let Some(url_str) = self.http { Some(Url::parse(&url_str)?) } else { None },
            https: if let Some(url_str) = self.https { Some(Url::parse(&url_str)?) } else { None },
            socks: if let Some(url_str) = self.socks { Some(ProxyConfig::socks5(Url::parse(&url_str)?)) } else { None },
        })
    }
}

impl ProxySettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_strings(http: Option<String>, https: Option<String>, socks: Option<String>) -> Result<Self, url::ParseError> {
        Ok(Self {
            http: if let Some(url_str) = http { Some(Url::parse(&url_str)?) } else { None },
            https: if let Some(url_str) = https { Some(Url::parse(&url_str)?) } else { None },
            socks: if let Some(url_str) = socks { Some(ProxyConfig::socks5(Url::parse(&url_str)?)) } else { None },
        })
    }

    pub fn with_http<S: Into<String>>(mut self, url: S) -> Result<Self, url::ParseError> {
        self.http = Some(Url::parse(&url.into())?);
        Ok(self)
    }

    pub fn with_https<S: Into<String>>(mut self, url: S) -> Result<Self, url::ParseError> {
        self.https = Some(Url::parse(&url.into())?);
        Ok(self)
    }

    pub fn with_socks<S: Into<String>>(mut self, url: S) -> Result<Self, url::ParseError> {
        self.socks = Some(ProxyConfig::socks5(Url::parse(&url.into())?));
        Ok(self)
    }
}

/// Macro to easily create ProxySettings from string literals
#[macro_export]
macro_rules! proxy_settings {
    (http = $http:expr) => {
        ProxySettings::new().with_http($http)
    };
    (https = $https:expr) => {
        ProxySettings::new().with_https($https)
    };
    (socks = $socks:expr) => {
        ProxySettings::new().with_socks($socks)
    };
    (http = $http:expr, https = $https:expr) => {
        ProxySettings::new().with_http($http)?.with_https($https)
    };
    (http = $http:expr, socks = $socks:expr) => {
        ProxySettings::new().with_http($http)?.with_socks($socks)
    };
    (https = $https:expr, socks = $socks:expr) => {
        ProxySettings::new().with_https($https)?.with_socks($socks)
    };
    (http = $http:expr, https = $https:expr, socks = $socks:expr) => {
        ProxySettings::new().with_http($http)?.with_https($https)?.with_socks($socks)
    };
}

// Simple struct initialization with string parsing
impl ProxySettings {
    pub fn parse(http: Option<&str>, https: Option<&str>, socks: Option<&str>) -> Result<Self, url::ParseError> {
        Ok(Self {
            http: if let Some(url_str) = http { Some(Url::parse(url_str)?) } else { None },
            https: if let Some(url_str) = https { Some(Url::parse(url_str)?) } else { None },
            socks: if let Some(url_str) = socks { Some(ProxyConfig::socks5(Url::parse(url_str)?)) } else { None },
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientTimeouts {
    pub connect: Option<Duration>,
    pub read: Option<Duration>,
    pub write: Option<Duration>,
}

impl Default for ClientTimeouts {
    fn default() -> Self {
        Self {
            connect: Some(Duration::from_secs(10)),
            read: Some(Duration::from_secs(30)),
            write: Some(Duration::from_secs(30)),
        }
    }
}

impl ClientTimeouts {
    pub fn disabled() -> Self {
        Self {
            connect: None,
            read: None,
            write: None,
        }
    }
}

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
        })
    }

    pub fn with_header(mut self, header: Header) -> Self {
        self.headers.push(header);
        self
    }

    pub fn with_headers(mut self, headers: Vec<Header>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_body<B: Into<Bytes>>(mut self, body: B) -> Self {
        self.body = Some(body.into());
        self.json = None;
        self
    }

    pub fn with_optional_body<B: Into<Bytes>>(mut self, body: Option<B>) -> Self {
        self.body = body.map(Into::into);
        if self.body.is_some() {
            self.json = None;
        }
        self
    }

    pub fn with_trailers(mut self, trailers: Option<Vec<Header>>) -> Self {
        self.trailers = trailers;
        self
    }

    pub fn with_params<I, K, V>(mut self, params: I) -> Self
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

    pub fn with_json(mut self, json: Value) -> Self {
        let serialized =
            serde_json::to_vec(&json).expect("serializing JSON body into bytes must succeed");
        self.body = Some(Bytes::from(serialized));
        self.json = Some(json);
        self
    }

    pub fn with_cookies<I, K, V>(mut self, cookies: I) -> Self
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

    pub fn with_timeout(mut self, timeouts: ClientTimeouts) -> Self {
        self.timeout = Some(timeouts);
        self
    }

    pub fn with_allow_redirects(mut self, allow: bool) -> Self {
        self.allow_redirects = allow;
        self
    }

    pub fn with_proxies(mut self, proxies: ProxySettings) -> Self {
        self.proxies = Some(proxies);
        self
    }

    pub fn with_http_proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.http = Some(Url::parse(proxy_url.as_ref())?);
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn with_https_proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.https = Some(Url::parse(proxy_url.as_ref())?);
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn with_socks5_proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.socks = Some(ProxyConfig::socks5(Url::parse(proxy_url.as_ref())?));
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn with_socks5_proxy_auth<S: AsRef<str>>(mut self, proxy_url: S, username: String, password: String) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.socks = Some(ProxyConfig::socks5(Url::parse(proxy_url.as_ref())?).with_auth(username, password));
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn with_socks4_proxy<S: AsRef<str>>(mut self, proxy_url: S) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.socks = Some(ProxyConfig::socks4(Url::parse(proxy_url.as_ref())?));
        self.proxies = Some(proxies);
        Ok(self)
    }

    pub fn with_socks4_proxy_auth<S: AsRef<str>>(mut self, proxy_url: S, username: String) -> Result<Self, url::ParseError> {
        let mut proxies = self.proxies.unwrap_or_default();
        proxies.socks = Some(ProxyConfig::socks4(Url::parse(proxy_url.as_ref())?).with_auth(username, String::new()));
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
            let estimated_param_size: usize = self.params.iter()
                .map(|(k, v)| k.len() + v.len() + 3) // +3 for =, &, and URL encoding overhead
                .sum();

            let total_capacity = path.len() +
                existing_query.map(|q| q.len() + 1).unwrap_or(0) +
                estimated_param_size + 10; // +10 buffer for URL encoding

            let mut serializer = form_urlencoded::Serializer::new(String::with_capacity(estimated_param_size));
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
        let additional_headers =
            (if self.json.is_some() && !Self::has_header_case_insensitive(&self.headers, "content-type") { 1 } else { 0 }) +
            (if !self.cookies.is_empty() && !Self::has_header_case_insensitive(&self.headers, "cookie") { 1 } else { 0 });

        let mut headers = Vec::with_capacity(self.headers.len() + additional_headers);
        headers.extend_from_slice(&self.headers);

        if self.json.is_some() && !Self::has_header_case_insensitive(&headers, crate::utils::CONTENT_TYPE_HEADER) {
            headers.push(Header::new(
                crate::utils::CONTENT_TYPE_HEADER.to_string(),
                crate::utils::APPLICATION_JSON.to_string(),
            ));
        }

        if let Some(cookie_value) = self.cookie_header_value() {
            if !Self::has_header_case_insensitive(&headers, crate::utils::COOKIE_HEADER) {
                headers.push(Header::new(crate::utils::COOKIE_HEADER.to_string(), cookie_value));
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
            let estimated_size: usize = self.cookies.iter()
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
