use crate::types::{Frame, FrameType, FrameTypeH2, Header, ProtocolError};
use bytes::{BufMut, Bytes, BytesMut};
use hpack::{Decoder, Encoder};

// HTTP/2 Frame Format (RFC 7540 Section 4.1):
//  0                   1                   2                   3
//  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
// |                 Length (24)                   |
// +---------------+---------------+---------------+
// |   Type (8)    |   Flags (8)   |
// +-+-+-----------+---------------+-------------------------------+
// |R|                 Stream Identifier (31)                      |
// +=+=============================================================+
// |                   Frame Payload (0...)                      ...
// +---------------------------------------------------------------+

pub const FRAME_HEADER_SIZE: usize = 9;

// HTTP/2 Frame Type Constants
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

// HTTP/2 Frame Flags
pub const END_STREAM_FLAG: u8 = 0x1; // DATA, HEADERS
pub const ACK_FLAG: u8 = 0x1; // SETTINGS, PING
pub const END_HEADERS_FLAG: u8 = 0x4; // HEADERS, PUSH_PROMISE, CONTINUATION
pub const PADDED_FLAG: u8 = 0x8; // DATA, HEADERS, PUSH_PROMISE
pub const PRIORITY_FLAG: u8 = 0x20; // HEADERS

// Maximum frame size (RFC 7540 Section 4.2)
pub const DEFAULT_MAX_FRAME_SIZE: u32 = 16384; // 2^14
pub const MAX_FRAME_SIZE_UPPER_BOUND: u32 = 16777215; // 2^24 - 1

impl Frame {
    pub fn new(frame_type: FrameTypeH2, flags: u8, stream_id: u32, payload: Bytes) -> Self {
        Self {
            frame_type: FrameType::H2(frame_type),
            flags,
            stream_id,
            payload,
        }
    }

    pub fn data(stream_id: u32, data: Bytes, end_stream: bool) -> Self {
        let flags = if end_stream { END_STREAM_FLAG } else { 0 };
        Self::new(FrameTypeH2::Data, flags, stream_id, data)
    }

    pub fn headers(
        stream_id: u32,
        headers: &[Header],
        end_stream: bool,
        end_headers: bool,
    ) -> Result<Self, ProtocolError> {
        let mut flags = 0;
        if end_stream {
            flags |= END_STREAM_FLAG;
        }
        if end_headers {
            flags |= END_HEADERS_FLAG;
        }

        let payload = Self::encode_headers_hpack(headers)?;
        Ok(Self::new(FrameTypeH2::Headers, flags, stream_id, payload))
    }

    pub fn settings(settings: &[(u16, u32)]) -> Self {
        let mut payload = BytesMut::new();
        for &(id, value) in settings {
            payload.put_u16(id);
            payload.put_u32(value);
        }
        Self::new(FrameTypeH2::Settings, 0, 0, payload.freeze())
    }

    pub fn settings_ack() -> Self {
        Self::new(FrameTypeH2::Settings, ACK_FLAG, 0, Bytes::new())
    }

    pub fn window_update(stream_id: u32, increment: u32) -> Result<Self, ProtocolError> {
        let mut payload = BytesMut::with_capacity(4);
        payload.put_u32(increment & 0x7FFFFFFF);
        Ok(Self::new(
            FrameTypeH2::WindowUpdate,
            0,
            stream_id,
            payload.freeze(),
        ))
    }

    pub fn rst(stream_id: u32, error_code: u32) -> Self {
        let mut payload = BytesMut::with_capacity(4);
        payload.put_u32(error_code);
        Self::new(FrameTypeH2::RstStream, 0, stream_id, payload.freeze())
    }

    pub fn ping(data: [u8; 8]) -> Self {
        let payload = Bytes::copy_from_slice(&data);
        Self::new(FrameTypeH2::Ping, 0, 0, payload)
    }

    pub fn ping_ack(data: [u8; 8]) -> Self {
        let payload = Bytes::copy_from_slice(&data);
        Self::new(FrameTypeH2::Ping, ACK_FLAG, 0, payload)
    }

    pub fn goaway(last_stream_id: u32, error_code: u32, debug_data: Option<&[u8]>) -> Self {
        let mut payload = BytesMut::with_capacity(8 + debug_data.map(|d| d.len()).unwrap_or(0));

        payload.put_u32(last_stream_id & 0x7FFFFFFF);
        payload.put_u32(error_code);

        if let Some(debug) = debug_data {
            payload.put_slice(debug);
        }

        Self::new(FrameTypeH2::GoAway, 0, 0, payload.freeze())
    }

    pub fn continuation(
        stream_id: u32,
        headers: &[Header],
        end_headers: bool,
    ) -> Result<Self, ProtocolError> {
        let flags = if end_headers { END_HEADERS_FLAG } else { 0 };
        let payload = Self::encode_headers_hpack(headers)?;
        Ok(Self::new(
            FrameTypeH2::Continuation,
            flags,
            stream_id,
            payload,
        ))
    }

    pub fn priority(
        stream_id: u32,
        length: usize,
        dependency: u32,
        weight: u8,
        exclusive: bool,
    ) -> Result<Self, ProtocolError> {
        let mut payload = BytesMut::with_capacity(length);

        let dependency = if exclusive {
            dependency | 0x80000000
        } else {
            dependency & 0x7FFFFFFF
        };

        payload.put_u32(dependency);
        payload.put_u8(weight);

        Ok(Self::new(
            FrameTypeH2::Priority,
            0,
            stream_id,
            payload.freeze(),
        ))
    }

    fn encode_headers_hpack(headers: &[Header]) -> Result<Bytes, ProtocolError> {
        let mut encoder = Encoder::new();

        let header_tuples: Vec<(&[u8], &[u8])> = headers
            .iter()
            .map(|h| {
                let name = h.name.as_bytes();
                let value = h.value.as_ref().map(|v| v.as_bytes()).unwrap_or(&[]);
                (name, value)
            })
            .collect();

        let encoded = encoder.encode(header_tuples);
        Ok(Bytes::from(encoded))
    }

    fn decode_headers_hpack(payload: &[u8]) -> Result<Vec<Header>, ProtocolError> {
        let mut decoder = Decoder::new();

        match decoder.decode(payload) {
            Ok(header_list) => {
                let headers = header_list
                    .into_iter()
                    .map(|(name, value)| {
                        let name_str = String::from_utf8(name).map_err(|e| {
                            ProtocolError::HeaderEncodingError(
                                format!("Invalid UTF-8 in header name: {}", e)
                            )
                        })?;
                        let value_str = if value.is_empty() {
                            None
                        } else {
                            Some(String::from_utf8(value).map_err(|e| {
                                ProtocolError::HeaderEncodingError(
                                    format!("Invalid UTF-8 in header value: {}", e)
                                )
                            })?)
                        };
                        Ok(Header {
                            name: name_str,
                            value: value_str,
                        })
                    })
                    .collect::<Result<Vec<_>, ProtocolError>>()?;
                Ok(headers)
            }
            Err(e) => Err(ProtocolError::H2CompressionError(format!(
                "HPACK decode error: {:?}",
                e
            ))),
        }
    }

    pub fn get_frame_type_u8(&self) -> u8 {
        match &self.frame_type {
            FrameType::H2(frame_type) => match frame_type {
                FrameTypeH2::Data => DATA_FRAME_TYPE,
                FrameTypeH2::Headers => HEADERS_FRAME_TYPE,
                FrameTypeH2::Priority => PRIORITY_FRAME_TYPE,
                FrameTypeH2::RstStream => RST_STREAM_FRAME_TYPE,
                FrameTypeH2::Settings => SETTINGS_FRAME_TYPE,
                FrameTypeH2::PushPromise => PUSH_PROMISE_FRAME_TYPE,
                FrameTypeH2::Ping => PING_FRAME_TYPE,
                FrameTypeH2::GoAway => GOAWAY_FRAME_TYPE,
                FrameTypeH2::WindowUpdate => WINDOW_UPDATE_FRAME_TYPE,
                FrameTypeH2::Continuation => CONTINUATION_FRAME_TYPE,
            },
            FrameType::H3(_) => 0, // Not applicable for H2 framing
        }
    }

    pub fn is_end_stream(&self) -> bool {
        (self.flags & END_STREAM_FLAG) != 0
    }

    pub fn is_end_headers(&self) -> bool {
        (self.flags & END_HEADERS_FLAG) != 0
    }

    pub fn is_ack(&self) -> bool {
        (self.flags & ACK_FLAG) != 0
    }

    pub fn is_padded(&self) -> bool {
        (self.flags & PADDED_FLAG) != 0
    }

    pub fn has_priority(&self) -> bool {
        (self.flags & PRIORITY_FLAG) != 0
    }

    pub fn decode_headers(&self) -> Result<Vec<Header>, ProtocolError> {
        match &self.frame_type {
            FrameType::H2(FrameTypeH2::Headers) | FrameType::H2(FrameTypeH2::Continuation) => {
                Self::decode_headers_hpack(&self.payload)
            }
            _ => Err(ProtocolError::RequestFailed(
                "Frame is not a header frame".to_string(),
            )),
        }
    }

    pub fn serialize(&self) -> Result<Bytes, ProtocolError> {
        let frame_type_u8 = self.get_frame_type_u8();

        if self.payload.len() > MAX_FRAME_SIZE_UPPER_BOUND as usize {
            return Err(ProtocolError::H2FrameSizeError(
                format!("Frame payload size {} exceeds maximum {}",
                    self.payload.len(), MAX_FRAME_SIZE_UPPER_BOUND)
            ));
        }

        let mut result = BytesMut::with_capacity(FRAME_HEADER_SIZE + self.payload.len());

        // Length (24 bits)
        let length = self.payload.len() as u32;
        result.put_u8(((length >> 16) & 0xFF) as u8);
        result.put_u8(((length >> 8) & 0xFF) as u8);
        result.put_u8((length & 0xFF) as u8);

        // Type (8 bits)
        result.put_u8(frame_type_u8);

        // Flags (8 bits)
        result.put_u8(self.flags);

        // Stream ID (31 bits, with reserved bit clear)
        result.put_u8(((self.stream_id >> 24) & 0x7F) as u8); // Clear reserved bit
        result.put_u8(((self.stream_id >> 16) & 0xFF) as u8);
        result.put_u8(((self.stream_id >> 8) & 0xFF) as u8);
        result.put_u8((self.stream_id & 0xFF) as u8);

        // Payload
        result.put_slice(&self.payload);

        Ok(result.freeze())
    }

    pub fn parse(data: &[u8]) -> Result<Self, ProtocolError> {
        if data.len() < FRAME_HEADER_SIZE {
            return Err(ProtocolError::InvalidResponse(
                "Frame too short".to_string(),
            ));
        }

        // Parse Length (24 bits)
        let length = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32);

        // Parse Type (8 bits)
        let frame_type_u8 = data[3];

        // Parse Flags (8 bits)
        let flags = data[4];

        // Parse Stream Identifier (31 bits, R bit reserved)
        let stream_id = ((data[5] as u32) << 24)
            | ((data[6] as u32) << 16)
            | ((data[7] as u32) << 8)
            | (data[8] as u32);
        let stream_id = stream_id & 0x7FFFFFFF; // Clear the reserved bit

        if data.len() < FRAME_HEADER_SIZE + length as usize {
            return Err(ProtocolError::InvalidResponse(
                "Incomplete frame payload".to_string(),
            ));
        }

        let frame_type = match frame_type_u8 {
            DATA_FRAME_TYPE => FrameTypeH2::Data,
            HEADERS_FRAME_TYPE => FrameTypeH2::Headers,
            PRIORITY_FRAME_TYPE => FrameTypeH2::Priority,
            RST_STREAM_FRAME_TYPE => FrameTypeH2::RstStream,
            SETTINGS_FRAME_TYPE => FrameTypeH2::Settings,
            PUSH_PROMISE_FRAME_TYPE => FrameTypeH2::PushPromise,
            PING_FRAME_TYPE => FrameTypeH2::Ping,
            GOAWAY_FRAME_TYPE => FrameTypeH2::GoAway,
            WINDOW_UPDATE_FRAME_TYPE => FrameTypeH2::WindowUpdate,
            CONTINUATION_FRAME_TYPE => FrameTypeH2::Continuation,
            _ => {
                return Err(ProtocolError::InvalidResponse(format!(
                    "Unknown frame type: {}",
                    frame_type_u8
                )));
            }
        };

        let payload =
            Bytes::copy_from_slice(&data[FRAME_HEADER_SIZE..FRAME_HEADER_SIZE + length as usize]);

        Ok(Frame {
            frame_type: FrameType::H2(frame_type),
            flags,
            stream_id,
            payload,
        })
    }
}
