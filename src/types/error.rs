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
    InvalidProxy(String),
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
            ProtocolError::InvalidProxy(msg) => write!(f, "Invalid proxy: {}", msg),
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
