use crate::types::error::ProtocolError;
use url::Url;

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
        let username = url.username();
        let username = if username.is_empty() {
            None
        } else {
            Some(username.to_string())
        };
        let password = url.password().map(|value| value.to_string());

        Self {
            url,
            proxy_type,
            username,
            password,
        }
    }

    pub fn auth(mut self, username: String, password: String) -> Self {
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
            http: if let Some(url_str) = self.http {
                Some(Url::parse(&url_str)?)
            } else {
                None
            },
            https: if let Some(url_str) = self.https {
                Some(Url::parse(&url_str)?)
            } else {
                None
            },
            socks: if let Some(url_str) = self.socks {
                Some(ProxyConfig::socks5(Url::parse(&url_str)?))
            } else {
                None
            },
        })
    }
}

impl ProxySettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_strings(
        http: Option<String>,
        https: Option<String>,
        socks: Option<String>,
    ) -> Result<Self, url::ParseError> {
        Ok(Self {
            http: if let Some(url_str) = http {
                Some(Url::parse(&url_str)?)
            } else {
                None
            },
            https: if let Some(url_str) = https {
                Some(Url::parse(&url_str)?)
            } else {
                None
            },
            socks: if let Some(url_str) = socks {
                Some(ProxyConfig::socks5(Url::parse(&url_str)?))
            } else {
                None
            },
        })
    }

    pub fn http<S: Into<String>>(mut self, url: S) -> Result<Self, url::ParseError> {
        self.http = Some(Url::parse(&url.into())?);
        Ok(self)
    }

    pub fn https<S: Into<String>>(mut self, url: S) -> Result<Self, url::ParseError> {
        self.https = Some(Url::parse(&url.into())?);
        Ok(self)
    }

    pub fn socks<S: Into<String>>(mut self, url: S) -> Result<Self, url::ParseError> {
        self.socks = Some(ProxyConfig::socks5(Url::parse(&url.into())?));
        Ok(self)
    }
}

/// Macro to easily create ProxySettings from string literals
#[macro_export]
macro_rules! proxy_settings {
    (http = $http:expr) => {
        ProxySettings::new().http($http)
    };
    (https = $https:expr) => {
        ProxySettings::new().https($https)
    };
    (socks = $socks:expr) => {
        ProxySettings::new().socks($socks)
    };
    (http = $http:expr, https = $https:expr) => {
        ProxySettings::new().http($http)?.https($https)
    };
    (http = $http:expr, socks = $socks:expr) => {
        ProxySettings::new().http($http)?.socks($socks)
    };
    (https = $https:expr, socks = $socks:expr) => {
        ProxySettings::new().https($https)?.socks($socks)
    };
    (http = $http:expr, https = $https:expr, socks = $socks:expr) => {
        ProxySettings::new()
            .http($http)?
            .https($https)?
            .socks($socks)
    };
}

// Simple struct initialization with string parsing
impl ProxySettings {
    pub fn parse(
        http: Option<&str>,
        https: Option<&str>,
        socks: Option<&str>,
    ) -> Result<Self, url::ParseError> {
        Ok(Self {
            http: if let Some(url_str) = http {
                Some(Url::parse(url_str)?)
            } else {
                None
            },
            https: if let Some(url_str) = https {
                Some(Url::parse(url_str)?)
            } else {
                None
            },
            socks: if let Some(url_str) = socks {
                Some(ProxyConfig::socks5(Url::parse(url_str)?))
            } else {
                None
            },
        })
    }

    pub fn set_proxy_url(&mut self, url: Url) -> Result<(), ProtocolError> {
        let scheme = url.scheme().to_ascii_lowercase();
        match scheme.as_str() {
            "http" => {
                self.http = Some(url);
            }
            "https" => {
                self.https = Some(url);
            }
            "socks" | "socks5" => {
                self.socks = Some(ProxyConfig::socks5(url));
            }
            "socks4" => {
                self.socks = Some(ProxyConfig::socks4(url));
            }
            other => {
                return Err(ProtocolError::InvalidProxy(format!(
                    "Unsupported proxy scheme '{}'",
                    other
                )));
            }
        }
        Ok(())
    }

    pub fn set_proxy(&mut self, proxy: &str) -> Result<(), ProtocolError> {
        let parsed = Url::parse(proxy)
            .map_err(|err| ProtocolError::InvalidProxy(format!("{} ({})", proxy, err)))?;
        self.set_proxy_url(parsed)
    }
}
