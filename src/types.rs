use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HttpProtocol {
    Http1,
    Http2,
    Http3,
}

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

    pub fn path(&self) -> String {
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

    pub fn path_only(&self) -> &str {
        let path = self.url.path();
        if path.is_empty() {
            "/"
        } else {
            path
        }
    }

    pub fn as_str(&self) -> &str {
        self.url.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct Header {
    pub name: String,
    pub value: Option<String>,
}

impl Header {
    pub fn new(name: String, value: String) -> Self {
        Self {
            name,
            value: Some(value),
        }
    }

    pub fn new_valueless(name: String) -> Self {
        Self { name, value: None }
    }

    pub fn to_string(&self) -> String {
        if let Some(ref value) = self.value {
            format!("{}: {}", self.name, value)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub protocol_version: String,
    pub headers: Vec<Header>,
    pub body: Bytes,
    pub trailers: Option<Vec<Header>>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            protocol_version: "HTTP/1.1".to_string(),
            headers: Vec::new(),
            body: Bytes::new(),
            trailers: None,
        }
    }

    pub fn new_with_protocol(status: u16, protocol_version: String) -> Self {
        Self {
            status,
            protocol_version,
            headers: Vec::new(),
            body: Bytes::new(),
            trailers: None,
        }
    }
}

/// Builder-style representation of an HTTP request used across protocol clients.
///
/// Bodies are stored as raw [`bytes::Bytes`] so callers can decide the encoding.
/// Helper methods allow optional bodies and trailers to be attached fluently.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub headers: Vec<Header>,
    pub body: Option<Bytes>,
    pub trailers: Option<Vec<Header>>,
}

impl Request {
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            headers: Vec::new(),
            body: None,
            trailers: None,
        }
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
        self
    }

    pub fn with_optional_body<B: Into<Bytes>>(mut self, body: Option<B>) -> Self {
        self.body = body.map(Into::into);
        self
    }

    pub fn with_trailers(mut self, trailers: Option<Vec<Header>>) -> Self {
        self.trailers = trailers;
        self
    }
}

#[async_trait]
pub trait Protocol {
    /// Send `request` to the provided `target`, returning the protocol-specific [`Response`].
    async fn send(&self, target: &Target, request: Request) -> Result<Response, ProtocolError>;
}

#[derive(Debug)]
pub enum ProtocolError {
    ConnectionFailed(String),
    RequestFailed(String),
    InvalidResponse(String),
    Timeout,
    Io(std::io::Error),
    H2StreamError(String),
    H3StreamError(String),
    H3MessageError(String),
    MalformedHeaders(String),
    InvalidMethod(String),
    InvalidTarget(String),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            ProtocolError::RequestFailed(msg) => write!(f, "Request failed: {}", msg),
            ProtocolError::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            ProtocolError::Timeout => write!(f, "Request timeout"),
            ProtocolError::Io(err) => write!(f, "IO error: {}", err),
            ProtocolError::H2StreamError(msg) => write!(f, "HTTP/2 stream error: {}", msg),
            ProtocolError::H3StreamError(msg) => write!(f, "HTTP/3 stream error: {}", msg),
            ProtocolError::H3MessageError(msg) => write!(f, "HTTP/3 message error: {}", msg),
            ProtocolError::MalformedHeaders(msg) => write!(f, "Malformed headers: {}", msg),
            ProtocolError::InvalidMethod(msg) => write!(f, "Invalid method: {}", msg),
            ProtocolError::InvalidTarget(msg) => write!(f, "Invalid target: {}", msg),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone)]
pub enum FrameType {
    H2(FrameTypeH2),
    H3(FrameTypeH3),
}

#[derive(Debug, Clone)]
pub enum FrameTypeH2 {
    Data,         // 0x0
    Headers,      // 0x1
    Priority,     // 0x2
    RstStream,    // 0x3
    Settings,     // 0x4
    PushPromise,  // 0x5
    Ping,         // 0x6
    GoAway,       // 0x7
    WindowUpdate, // 0x8
    Continuation, // 0x9
}

#[derive(Debug, Clone)]
pub enum FrameTypeH3 {
    Data,        // 0x0
    Headers,     // 0x1
    CancelPush,  // 0x3
    Settings,    // 0x4
    PushPromise, // 0x5
    GoAway,      // 0x7
    MaxPushId,   // 0xd
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub frame_type: FrameType,
    pub flags: u8,
    pub stream_id: u32,
    pub payload: Bytes,
}
