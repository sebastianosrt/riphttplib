#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RstErrorCode {
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
    ConnectError = 0xA,
    EnhanceYourCalm = 0xB,
    InadequateSecurity = 0xC,
    Http11Required = 0xD,
}

impl RstErrorCode {
    pub fn description(&self) -> &'static str {
        match self {
            RstErrorCode::NoError => "Graceful shutdown of the stream; no error occurred.",
            RstErrorCode::ProtocolError => {
                "Generic protocol error; indicates an error in adherence to the HTTP/2 framing or protocol rules."
            }
            RstErrorCode::InternalError => {
                "Implementation fault in the HTTP/2 endpoint (e.g., an internal logic failure)."
            }
            RstErrorCode::FlowControlError => "Stream exceeded its flow-control window limits.",
            RstErrorCode::SettingsTimeout => "A SETTINGS frame was sent but not acknowledged in time.",
            RstErrorCode::StreamClosed => {
                "A frame was received for a stream that has already been closed."
            }
            RstErrorCode::FrameSizeError => "Frame size incorrect, violating protocol limits.",
            RstErrorCode::RefusedStream => {
                "Stream was refused before processing, often due to resource exhaustion or policy."
            }
            RstErrorCode::Cancel => {
                "Endpoint cancels the stream for reasons unrelated to errors, such as user actions."
            }
            RstErrorCode::CompressionError => {
                "Compression context error, often in header compression (HPACK)."
            }
            RstErrorCode::ConnectError => "TCP connection error specifically for the CONNECT method.",
            RstErrorCode::EnhanceYourCalm => {
                "Endpoint is overwhelmed; client should reduce request rate to avoid being throttled."
            }
            RstErrorCode::InadequateSecurity => {
                "Negotiated TLS parameters are unacceptable, and the connection must close."
            }
            RstErrorCode::Http11Required => {
                "The resource must be accessed using HTTP/1.1, not HTTP/2."
            }
        }
    }
}

impl From<u32> for RstErrorCode {
    fn from(code: u32) -> Self {
        match code {
            0x0 => RstErrorCode::NoError,
            0x1 => RstErrorCode::ProtocolError,
            0x2 => RstErrorCode::InternalError,
            0x3 => RstErrorCode::FlowControlError,
            0x4 => RstErrorCode::SettingsTimeout,
            0x5 => RstErrorCode::StreamClosed,
            0x6 => RstErrorCode::FrameSizeError,
            0x7 => RstErrorCode::RefusedStream,
            0x8 => RstErrorCode::Cancel,
            0x9 => RstErrorCode::CompressionError,
            0xA => RstErrorCode::ConnectError,
            0xB => RstErrorCode::EnhanceYourCalm,
            0xC => RstErrorCode::InadequateSecurity,
            0xD => RstErrorCode::Http11Required,
            _ => RstErrorCode::InternalError,
        }
    }
}

impl From<RstErrorCode> for u32 {
    fn from(code: RstErrorCode) -> Self {
        code as u32
    }
}
