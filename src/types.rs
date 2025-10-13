use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HttpProtocol {
    Http1,
    Http2,
    Http3,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub url: String,
    pub protocols: HashSet<HttpProtocol>,
    pub scheme: String,
    pub path: String,
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

#[async_trait]
pub trait Protocol {
    // async fn send(&self, target: &Target, request: Request) -> Result<Response, ProtocolError>;
    async fn send(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError>;
    async fn get(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError>;
    async fn post(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError>;
    async fn send_bytes(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError>;
    async fn get_bytes(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError>;
    async fn post_bytes(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError>;
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
