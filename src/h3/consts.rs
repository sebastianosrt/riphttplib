/// Default HTTP/3 settings (matching RFC 9114 recommendations).
pub const DEFAULT_QPACK_MAX_TABLE_CAPACITY: u64 = 0;
pub const DEFAULT_MAX_FIELD_SECTION_SIZE: u64 = 8_192;
pub const DEFAULT_QPACK_BLOCKED_STREAMS: u64 = 0;

/// Unidirectional stream type identifiers (RFC 9114 §6.2).
pub const CONTROL_STREAM_TYPE: u64 = 0x00;
pub const QPACK_ENCODER_STREAM_TYPE: u64 = 0x02;
pub const QPACK_DECODER_STREAM_TYPE: u64 = 0x03;

/// Client-initiated bidirectional stream IDs increment by four (RFC 9000 §2.1).
pub const CLIENT_BIDI_STREAM_INCREMENT: u32 = 4;
