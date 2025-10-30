use super::protocol::HttpProtocol;
use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone)]
pub struct Target {
    pub url: Url,
    pub protocols: HashSet<HttpProtocol>,
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

    pub fn as_str(&self) -> &str {
        self.url.as_ref()
    }

    // async fn test_http1_support(&self, timeouts: &ClientTimeouts) -> bool {
    //     match Request::new(self.as_str(), "HEAD") {
    //         Ok(request) => {
    //             let client = H1::timeouts(timeouts.clone());
    //             client.send_request(request).await.is_ok()
    //         }
    //         Err(_) => false,
    //     }
    // }

    // async fn test_http2_support(&self, timeouts: &ClientTimeouts) -> bool {
    //     match Request::new(self.as_str(), "HEAD") {
    //         Ok(request) => {
    //             let client = H2::timeouts(timeouts.clone());
    //             client.send_request(request).await.is_ok()
    //         }
    //         Err(_) => false,
    //     }
    // }

    // async fn test_http3_support(&self, timeouts: &ClientTimeouts) -> bool {
    //     // Only test HTTP/3 for HTTPS targets (QUIC requires TLS)
    //     if self.scheme() != "https" {
    //         return false;
    //     }

    //     match Request::new(self.as_str(), "HEAD") {
    //         Ok(request) => {
    //             let client = H3::timeouts(timeouts.clone());
    //             client.send_request(request).await.is_ok()
    //         }
    //         Err(_) => false,
    //     }
    // }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.url.as_str())
    }
}
