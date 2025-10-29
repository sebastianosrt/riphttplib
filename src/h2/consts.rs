/// HTTP/2 connection preface (RFC 7540 Section 3.5)
pub const CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// HTTP/2 Settings Parameters (RFC 7540 Section 6.5.2)
pub const SETTINGS_HEADER_TABLE_SIZE: u16 = 0x1;
pub const SETTINGS_ENABLE_PUSH: u16 = 0x2;
pub const SETTINGS_MAX_CONCURRENT_STREAMS: u16 = 0x3;
pub const SETTINGS_INITIAL_WINDOW_SIZE: u16 = 0x4;
pub const SETTINGS_MAX_FRAME_SIZE: u16 = 0x5;
pub const SETTINGS_MAX_HEADER_LIST_SIZE: u16 = 0x6;

/// Default Settings Values
pub const DEFAULT_HEADER_TABLE_SIZE: u32 = 0;
pub const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 100;
pub const DEFAULT_INITIAL_WINDOW_SIZE: u32 = 65_535;
pub const DEFAULT_MAX_FRAME_SIZE: u32 = 16_384;
pub const DEFAULT_MAX_HEADER_LIST_SIZE: u32 = 8_192;

/// Frame/flag constants (RFC 7540 Section 4)
pub const FRAME_HEADER_SIZE: usize = 9;

pub const DATA_FRAME_TYPE: u8 = 0x0;
pub const HEADERS_FRAME_TYPE: u8 = 0x1;
pub const PRIORITY_FRAME_TYPE: u8 = 0x2;
pub const RST_STREAM_FRAME_TYPE: u8 = 0x3;
pub const SETTINGS_FRAME_TYPE: u8 = 0x4;
pub const PUSH_PROMISE_FRAME_TYPE: u8 = 0x5;
pub const PING_FRAME_TYPE: u8 = 0x6;
pub const GOAWAY_FRAME_TYPE: u8 = 0x7;
pub const WINDOW_UPDATE_FRAME_TYPE: u8 = 0x8;
pub const CONTINUATION_FRAME_TYPE: u8 = 0x9;

pub const END_STREAM_FLAG: u8 = 0x1;
pub const ACK_FLAG: u8 = 0x1;
pub const END_HEADERS_FLAG: u8 = 0x4;
pub const PADDED_FLAG: u8 = 0x8;
pub const PRIORITY_FLAG: u8 = 0x20;

pub const MAX_FRAME_SIZE_UPPER_BOUND: u32 = 16_777_215; // 2^24 - 1
