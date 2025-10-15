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

    // TODO rename to path_with_query
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

    // TODO consider removing
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
    // TODO maybe mergeable with the above
    pub fn with_optional_body<B: Into<Bytes>>(mut self, body: Option<B>) -> Self {
        self.body = body.map(Into::into);
        self
    }

    pub fn with_trailers(mut self, trailers: Option<Vec<Header>>) -> Self {
        self.trailers = trailers;
        self
    }
}

#[async_trait(?Send)]
pub trait Protocol {
    async fn send(&self, target: &Target, request: Request) -> Result<Response, ProtocolError>;
}

#[derive(Debug)]
pub enum ProtocolError {
    ConnectionFailed(String),
    RequestFailed(String),
    InvalidResponse(String),
    Timeout,
    Io(std::io::Error),

    // HTTP/2 specific errors
    H2FrameSizeError(String),
    H2FlowControlError(String),
    H2CompressionError(String),
    H2StreamError(H2StreamErrorKind),
    H2ConnectionError(H2ConnectionErrorKind),
    H2ProtocolError(String),

    // HTTP/3 specific errors
    H3StreamError(H3StreamErrorKind),
    H3MessageError(String),
    H3StreamCreationError(String),
    H3QpackError(String),
    H3ConnectionError(String),

    // Header handling errors
    HeaderEncodingError(String),
    MalformedHeaders(String),

    // Method and target errors
    InvalidMethod(String),
    InvalidTarget(String),
}

#[derive(Debug)]
pub enum H2StreamErrorKind {
    Reset(H2ErrorCode),
    FlowControlViolation,
    StreamClosed,
    InvalidState(String),
    ProtocolViolation(String),
}

#[derive(Debug)]
pub enum H2ConnectionErrorKind {
    GoAway(H2ErrorCode, String),
    SettingsTimeout,
    ProtocolViolation(String),
    CompressionFailure,
}

#[derive(Debug)]
pub enum H3StreamErrorKind {
    StreamClosed,
    InvalidState(String),
    FlowControlViolation,
    ProtocolViolation(String),
}

// HTTP/2 Error Codes (RFC 7540 Section 7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H2ErrorCode {
    NoError = 0x0,
    ProtocolError = 0x1,
    InternalError = 0x2,
    FlowControlError = 0x3,
    SettingsTimeout = 0x4,
    StreamClosed = 0x5,
    FrameSizeError = 0x6,
    RefusedStream = 0x7,
    Cancel = 0x8,
    CompressionError = 0x9,
    ConnectError = 0xa,
    EnhanceYourCalm = 0xb,
    InadequateSecurity = 0xc,
    Http11Required = 0xd,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            ProtocolError::RequestFailed(msg) => write!(f, "Request failed: {}", msg),
            ProtocolError::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            ProtocolError::Timeout => write!(f, "Request timeout"),
            ProtocolError::Io(err) => write!(f, "IO error: {}", err),

            // HTTP/2 specific errors
            ProtocolError::H2FrameSizeError(msg) => write!(f, "HTTP/2 frame size error: {}", msg),
            ProtocolError::H2FlowControlError(msg) => {
                write!(f, "HTTP/2 flow control error: {}", msg)
            }
            ProtocolError::H2CompressionError(msg) => {
                write!(f, "HTTP/2 compression error: {}", msg)
            }
            ProtocolError::H2StreamError(kind) => write!(f, "HTTP/2 stream error: {}", kind),
            ProtocolError::H2ConnectionError(kind) => {
                write!(f, "HTTP/2 connection error: {}", kind)
            }
            ProtocolError::H2ProtocolError(msg) => write!(f, "HTTP/2 protocol error: {}", msg),

            // HTTP/3 specific errors
            ProtocolError::H3StreamError(kind) => write!(f, "HTTP/3 stream error: {}", kind),
            ProtocolError::H3MessageError(msg) => write!(f, "HTTP/3 message error: {}", msg),
            ProtocolError::H3StreamCreationError(msg) => {
                write!(f, "HTTP/3 stream creation error: {}", msg)
            }
            ProtocolError::H3QpackError(msg) => write!(f, "HTTP/3 QPACK error: {}", msg),
            ProtocolError::H3ConnectionError(msg) => write!(f, "HTTP/3 connection error: {}", msg),

            // Header handling errors
            ProtocolError::HeaderEncodingError(msg) => write!(f, "Header encoding error: {}", msg),
            ProtocolError::MalformedHeaders(msg) => write!(f, "Malformed headers: {}", msg),

            // Method and target errors
            ProtocolError::InvalidMethod(msg) => write!(f, "Invalid method: {}", msg),
            ProtocolError::InvalidTarget(msg) => write!(f, "Invalid target: {}", msg),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProtocolError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl std::fmt::Display for H2StreamErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            H2StreamErrorKind::Reset(code) => write!(f, "stream reset with error code {:?}", code),
            H2StreamErrorKind::FlowControlViolation => write!(f, "flow control violation"),
            H2StreamErrorKind::StreamClosed => write!(f, "stream closed"),
            H2StreamErrorKind::InvalidState(msg) => write!(f, "invalid stream state: {}", msg),
            H2StreamErrorKind::ProtocolViolation(msg) => write!(f, "protocol violation: {}", msg),
        }
    }
}

impl std::fmt::Display for H2ConnectionErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            H2ConnectionErrorKind::GoAway(code, debug) => {
                write!(
                    f,
                    "connection terminated with GOAWAY ({:?}): {}",
                    code, debug
                )
            }
            H2ConnectionErrorKind::SettingsTimeout => write!(f, "settings acknowledgment timeout"),
            H2ConnectionErrorKind::ProtocolViolation(msg) => {
                write!(f, "protocol violation: {}", msg)
            }
            H2ConnectionErrorKind::CompressionFailure => write!(f, "header compression failure"),
        }
    }
}

impl std::fmt::Display for H3StreamErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            H3StreamErrorKind::StreamClosed => write!(f, "stream closed"),
            H3StreamErrorKind::InvalidState(msg) => write!(f, "invalid stream state: {}", msg),
            H3StreamErrorKind::FlowControlViolation => write!(f, "flow control violation"),
            H3StreamErrorKind::ProtocolViolation(msg) => write!(f, "protocol violation: {}", msg),
        }
    }
}

impl std::fmt::Display for H2ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (name, description) = match self {
            H2ErrorCode::NoError => ("NO_ERROR", "graceful shutdown"),
            H2ErrorCode::ProtocolError => ("PROTOCOL_ERROR", "protocol error detected"),
            H2ErrorCode::InternalError => ("INTERNAL_ERROR", "implementation fault"),
            H2ErrorCode::FlowControlError => {
                ("FLOW_CONTROL_ERROR", "flow control protocol violated")
            }
            H2ErrorCode::SettingsTimeout => ("SETTINGS_TIMEOUT", "settings not acknowledged"),
            H2ErrorCode::StreamClosed => ("STREAM_CLOSED", "frame received for closed stream"),
            H2ErrorCode::FrameSizeError => ("FRAME_SIZE_ERROR", "frame size incorrect"),
            H2ErrorCode::RefusedStream => ("REFUSED_STREAM", "stream not processed"),
            H2ErrorCode::Cancel => ("CANCEL", "stream cancelled"),
            H2ErrorCode::CompressionError => ("COMPRESSION_ERROR", "compression state not updated"),
            H2ErrorCode::ConnectError => {
                ("CONNECT_ERROR", "TCP connection error for CONNECT method")
            }
            H2ErrorCode::EnhanceYourCalm => ("ENHANCE_YOUR_CALM", "processing capacity exceeded"),
            H2ErrorCode::InadequateSecurity => (
                "INADEQUATE_SECURITY",
                "negotiated TLS parameters inadequate",
            ),
            H2ErrorCode::Http11Required => ("HTTP_1_1_REQUIRED", "use HTTP/1.1 for request"),
        };
        write!(f, "{} (0x{:x}): {}", name, *self as u32, description)
    }
}

// From conversions
impl From<std::io::Error> for ProtocolError {
    fn from(err: std::io::Error) -> Self {
        ProtocolError::Io(err)
    }
}

impl From<H2StreamErrorKind> for ProtocolError {
    fn from(kind: H2StreamErrorKind) -> Self {
        ProtocolError::H2StreamError(kind)
    }
}

impl From<H2ConnectionErrorKind> for ProtocolError {
    fn from(kind: H2ConnectionErrorKind) -> Self {
        ProtocolError::H2ConnectionError(kind)
    }
}

impl From<H3StreamErrorKind> for ProtocolError {
    fn from(kind: H3StreamErrorKind) -> Self {
        ProtocolError::H3StreamError(kind)
    }
}

impl From<u32> for H2ErrorCode {
    fn from(code: u32) -> Self {
        match code {
            0x0 => H2ErrorCode::NoError,
            0x1 => H2ErrorCode::ProtocolError,
            0x2 => H2ErrorCode::InternalError,
            0x3 => H2ErrorCode::FlowControlError,
            0x4 => H2ErrorCode::SettingsTimeout,
            0x5 => H2ErrorCode::StreamClosed,
            0x6 => H2ErrorCode::FrameSizeError,
            0x7 => H2ErrorCode::RefusedStream,
            0x8 => H2ErrorCode::Cancel,
            0x9 => H2ErrorCode::CompressionError,
            0xa => H2ErrorCode::ConnectError,
            0xb => H2ErrorCode::EnhanceYourCalm,
            0xc => H2ErrorCode::InadequateSecurity,
            0xd => H2ErrorCode::Http11Required,
            _ => H2ErrorCode::InternalError, // Default for unknown error codes
        }
    }
}

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
    Unknown(u64),
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub frame_type: FrameType,
    pub flags: u8,
    pub stream_id: u32,
    pub payload: Bytes,
}

#[derive(Debug, Clone)]
pub struct FrameH2 {
    pub frame_type: FrameType,
    pub flags: u8,
    pub stream_id: u32,
    pub payload: Bytes,
}

#[derive(Debug, Clone)]
pub struct FrameH3 {
    pub frame_type: FrameType,
    pub stream_id: u32,
    pub payload: Bytes,
}

#[async_trait(?Send)]
pub trait FrameSink<F> {
    async fn write_frame(&mut self, frame: F) -> Result<(), ProtocolError>;
}

pub trait IntoFrameBatch<F> {
    fn into_batch(self) -> FrameBatch<F>;
}

impl<F> IntoFrameBatch<F> for FrameBatch<F> {
    fn into_batch(self) -> FrameBatch<F> {
        self
    }
}

impl<F> IntoFrameBatch<F> for F {
    fn into_batch(self) -> FrameBatch<F> {
        FrameBatch::new(vec![self])
    }
}

pub trait FrameBuilderExt<F>: Sized {
    fn repeat(self, count: usize) -> FrameBatch<F>;
    fn chain(self, other: impl IntoFrameBatch<F>) -> FrameBatch<F>;
    fn into_batch(self) -> FrameBatch<F>;
}

impl<F: Clone> FrameBuilderExt<F> for F {
    fn repeat(self, count: usize) -> FrameBatch<F> {
        FrameBatch {
            frames: vec![self; count],
        }
    }

    fn chain(self, other: impl IntoFrameBatch<F>) -> FrameBatch<F> {
        let mut batch = FrameBatch::new(vec![self]);
        batch.extend(other.into_batch());
        batch
    }

    fn into_batch(self) -> FrameBatch<F> {
        FrameBatch::new(vec![self])
    }
}

#[derive(Debug, Clone)]
pub struct FrameBatch<F> {
    frames: Vec<F>,
}

impl<F> IntoIterator for FrameBatch<F> {
    type Item = F;
    type IntoIter = std::vec::IntoIter<F>;

    fn into_iter(self) -> Self::IntoIter {
        self.frames.into_iter()
    }
}

impl<F> FrameBatch<F> {
    pub fn new(frames: Vec<F>) -> Self {
        Self { frames }
    }

    pub fn push(&mut self, frame: F) {
        self.frames.push(frame);
    }

    pub fn extend<I: IntoIterator<Item = F>>(&mut self, iter: I) {
        self.frames.extend(iter);
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn frames(&self) -> &[F] {
        &self.frames
    }

    pub fn into_frames(self) -> Vec<F> {
        self.frames
    }

    pub fn map<G, M>(self, mut map_fn: M) -> FrameBatch<G>
    where
        M: FnMut(F) -> G,
    {
        FrameBatch {
            frames: self.frames.into_iter().map(|f| map_fn(f)).collect(),
        }
    }

    pub fn chain(mut self, other: impl IntoFrameBatch<F>) -> FrameBatch<F> {
        let mut other = other.into_batch();
        self.frames.append(&mut other.frames);
        self
    }

    pub async fn send_all<S>(self, sink: &mut S) -> Result<(), ProtocolError>
    where
        S: FrameSink<F> + ?Sized,
    {
        for frame in self.frames {
            sink.write_frame(frame).await?;
        }
        Ok(())
    }

    pub async fn send<S>(self, sink: &mut S) -> Result<(), ProtocolError>
    where
        S: FrameSink<F> + ?Sized,
    {
        self.send_all(sink).await
    }
}
