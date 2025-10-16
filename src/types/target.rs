use std::collections::HashSet;
use url::Url;
use super::protocol::HttpProtocol;

#[derive(Debug, Clone)]
pub struct Target {
    pub url: Url,
    pub protocols: HashSet<HttpProtocol>
}

impl Target {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            protocols: HashSet::new(),
        }
    }

    pub fn scheme(&self) -> &str {
        self.url.scheme()
    }

    pub fn host(&self) -> Option<&str> {
        self.url.host_str()
    }

    pub fn port(&self) -> Option<u16> {
        self.url.port_or_known_default()
    }

    pub fn authority(&self) -> Option<String> {
        self.host().map(|host| match self.url.port() {
            Some(port) => format!("{}:{}", host, port),
            None => host.to_string(),
        })
    }

    pub fn path(&self) -> &str {
        let path = self.url.path();
        if path.is_empty() {
            "/"
        } else {
            path
        }
    }

    pub fn path_only(&self) -> &str {
        self.url.path()
    }

    pub fn path_query(&self) -> String {
        let mut value = self.url.path().to_string();
        if let Some(query) = self.url.query() {
            value.push('?');
            value.push_str(query);
        }
        if value.is_empty() {
            value.push('/');
        }
        value
    }

    pub fn as_str(&self) -> &str {
        self.url.as_ref()
    }
}


impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.url.as_str())
    }
}