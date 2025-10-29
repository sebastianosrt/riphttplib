use crate::types::{FrameH3, FrameSink, FrameType, FrameTypeH3, ProtocolError};
use bytes::{BufMut, Bytes, BytesMut};

// HTTP/3 Frame Types (RFC 9114 Section 7.2)
pub const DATA_FRAME_TYPE: u64 = 0x0;
pub const HEADERS_FRAME_TYPE: u64 = 0x1;
pub const CANCEL_PUSH_FRAME_TYPE: u64 = 0x3;
pub const SETTINGS_FRAME_TYPE: u64 = 0x4;
pub const PUSH_PROMISE_FRAME_TYPE: u64 = 0x5;
pub const GOAWAY_FRAME_TYPE: u64 = 0x7;
pub const MAX_PUSH_ID_FRAME_TYPE: u64 = 0x0d;

// HTTP/3 Settings Parameters (RFC 9114 Section 7.2.4.1)
pub const SETTINGS_QPACK_MAX_TABLE_CAPACITY: u64 = 0x1;
pub const SETTINGS_MAX_FIELD_SECTION_SIZE: u64 = 0x6;
pub const SETTINGS_QPACK_BLOCKED_STREAMS: u64 = 0x7;

impl FrameH3 {
    pub fn new(frame_type: FrameTypeH3, stream_id: u32, payload: Bytes) -> Self {
        Self {
            frame_type: FrameType::H3(frame_type),
            stream_id,
            payload,
        }
    }

    pub fn header(stream_id: u32, header_block: Bytes) -> Self {
        Self::new(FrameTypeH3::Headers, stream_id, header_block)
    }

    pub fn data(stream_id: u32, data: Bytes) -> Self {
        Self::new(FrameTypeH3::Data, stream_id, data)
    }

    pub fn settings(settings: &[(u64, u64)]) -> Self {
        let mut payload = BytesMut::new();
        for &(id, value) in settings {
            Self::encode_varint(&mut payload, id);
            Self::encode_varint(&mut payload, value);
        }
        Self::new(FrameTypeH3::Settings, 0, payload.freeze())
    }

    pub fn goaway(id: u64) -> Self {
        let mut payload = BytesMut::new();
        Self::encode_varint(&mut payload, id);
        Self::new(FrameTypeH3::GoAway, 0, payload.freeze())
    }

    pub fn max_push_id(push_id: u64) -> Self {
        let mut payload = BytesMut::new();
        Self::encode_varint(&mut payload, push_id);
        Self::new(FrameTypeH3::MaxPushId, 0, payload.freeze())
    }

    pub fn cancel_push(push_id: u64) -> Self {
        let mut payload = BytesMut::new();
        Self::encode_varint(&mut payload, push_id);
        Self::new(FrameTypeH3::CancelPush, 0, payload.freeze())
    }

    pub fn send<'a, S>(
        self,
        sink: &'a mut S,
    ) -> impl std::future::Future<Output = Result<(), ProtocolError>> + 'a
    where
        S: FrameSink<FrameH3> + 'a,
    {
        async move { sink.write_frame(self).await }
    }

    pub fn get_frame_type_u64(&self) -> u64 {
        match &self.frame_type {
            FrameType::H3(frame_type) => match frame_type {
                FrameTypeH3::Data => DATA_FRAME_TYPE,
                FrameTypeH3::Headers => HEADERS_FRAME_TYPE,
                FrameTypeH3::CancelPush => CANCEL_PUSH_FRAME_TYPE,
                FrameTypeH3::Settings => SETTINGS_FRAME_TYPE,
                FrameTypeH3::PushPromise => PUSH_PROMISE_FRAME_TYPE,
                FrameTypeH3::GoAway => GOAWAY_FRAME_TYPE,
                FrameTypeH3::MaxPushId => MAX_PUSH_ID_FRAME_TYPE,
                FrameTypeH3::Unknown(value) => *value,
            },
            FrameType::H2(_) => 0, // Not applicable for H3 framing
        }
    }

    pub fn serialize(&self) -> Result<Bytes, ProtocolError> {
        let frame_type = self.get_frame_type_u64();
        let length = self.payload.len() as u64;

        let mut result = BytesMut::new();

        // Encode frame type as varint
        Self::encode_varint(&mut result, frame_type);

        // Encode length as varint
        Self::encode_varint(&mut result, length);

        // Add payload
        result.put_slice(&self.payload);

        Ok(result.freeze())
    }

    pub fn parse(data: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if data.is_empty() {
            return Err(ProtocolError::InvalidResponse(
                "Empty frame data".to_string(),
            ));
        }

        let mut offset = 0;

        // Parse frame type (varint)
        let (frame_type_u64, consumed) = Self::decode_varint(&data[offset..]).ok_or_else(|| {
            ProtocolError::InvalidResponse("Invalid frame type varint".to_string())
        })?;
        offset += consumed;

        // Parse length (varint)
        let (length, consumed) = Self::decode_varint(&data[offset..])
            .ok_or_else(|| ProtocolError::InvalidResponse("Invalid length varint".to_string()))?;
        offset += consumed;

        if data.len() < offset + length as usize {
            return Err(ProtocolError::InvalidResponse(
                "Incomplete frame payload".to_string(),
            ));
        }

        let frame_type = match frame_type_u64 {
            DATA_FRAME_TYPE => FrameTypeH3::Data,
            HEADERS_FRAME_TYPE => FrameTypeH3::Headers,
            CANCEL_PUSH_FRAME_TYPE => FrameTypeH3::CancelPush,
            SETTINGS_FRAME_TYPE => FrameTypeH3::Settings,
            PUSH_PROMISE_FRAME_TYPE => FrameTypeH3::PushPromise,
            GOAWAY_FRAME_TYPE => FrameTypeH3::GoAway,
            MAX_PUSH_ID_FRAME_TYPE => FrameTypeH3::MaxPushId,
            other => FrameTypeH3::Unknown(other),
        };

        let payload = Bytes::copy_from_slice(&data[offset..offset + length as usize]);
        let total_consumed = offset + length as usize;

        Ok((
            FrameH3 {
                frame_type: FrameType::H3(frame_type),
                stream_id: 0, // Stream ID is handled at the QUIC layer in HTTP/3
                payload,
            },
            total_consumed,
        ))
    }

    // Variable-length integer encoding for HTTP/3 (RFC 9000 Section 16)
    fn encode_varint(buf: &mut BytesMut, value: u64) {
        if value < 0x40 {
            buf.put_u8(value as u8);
        } else if value < 0x4000 {
            buf.put_u16((value as u16) | 0x4000);
        } else if value < 0x40000000 {
            buf.put_u32((value as u32) | 0x80000000);
        } else {
            buf.put_u64(value | 0xC000000000000000);
        }
    }

    // Variable-length integer decoding for HTTP/3
    fn decode_varint(data: &[u8]) -> Option<(u64, usize)> {
        if data.is_empty() {
            return None;
        }

        let first_byte = data[0];
        let prefix = first_byte >> 6;

        match prefix {
            0 => Some((first_byte as u64, 1)),
            1 => {
                if data.len() < 2 {
                    return None;
                }
                let value = (((first_byte & 0x3F) as u16) << 8) | (data[1] as u16);
                Some((value as u64, 2))
            }
            2 => {
                if data.len() < 4 {
                    return None;
                }
                let value = (((first_byte & 0x3F) as u32) << 24)
                    | ((data[1] as u32) << 16)
                    | ((data[2] as u32) << 8)
                    | (data[3] as u32);
                Some((value as u64, 4))
            }
            3 => {
                if data.len() < 8 {
                    return None;
                }
                let value = (((first_byte & 0x3F) as u64) << 56)
                    | ((data[1] as u64) << 48)
                    | ((data[2] as u64) << 40)
                    | ((data[3] as u64) << 32)
                    | ((data[4] as u64) << 24)
                    | ((data[5] as u64) << 16)
                    | ((data[6] as u64) << 8)
                    | (data[7] as u64);
                Some((value, 8))
            }
            _ => None,
        }
    }
}
