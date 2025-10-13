use crate::h3::framing::{
    SETTINGS_MAX_FIELD_SECTION_SIZE, SETTINGS_QPACK_BLOCKED_STREAMS,
    SETTINGS_QPACK_MAX_TABLE_CAPACITY,
};
use crate::stream::{StreamType, create_quic_connection};
use crate::types::{Frame, FrameType, FrameTypeH3, Header, ProtocolError, Target};
use bytes::{Bytes, BytesMut};
use quinn::{Connection, RecvStream, SendStream};
use std::collections::HashMap;

// Default HTTP/3 Settings Values
pub const DEFAULT_QPACK_MAX_TABLE_CAPACITY: u64 = 0;
pub const DEFAULT_MAX_FIELD_SECTION_SIZE: u64 = 8192;
pub const DEFAULT_QPACK_BLOCKED_STREAMS: u64 = 0;

// Connection States
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Idle,
    Open,
    Closed,
}

// Stream States (Client perspective)
#[derive(Debug, Clone, PartialEq)]
pub enum StreamState {
    Idle,
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
}

pub struct StreamInfo {
    pub state: StreamState,
    // Store the receiving side for this HTTP/3 request stream.
    pub recv_stream: RecvStream,
    pub recv_buf: BytesMut,
}

pub struct H3Connection {
    pub connection: Connection,
    pub state: ConnectionState,
    pub settings: HashMap<u64, u64>,
    pub remote_settings: HashMap<u64, u64>,
    pub streams: HashMap<u32, StreamInfo>, // HTTP/3 request stream tracking
    pub next_stream_id: u32,
    pub control_send_stream: Option<SendStream>,
    pub control_recv_stream: Option<RecvStream>,
    // QPACK unidirectional streams (optional in this minimal implementation)
    pub qpack_encoder_send: Option<SendStream>,
    pub qpack_decoder_send: Option<SendStream>,
    pub qpack_encoder_recv: Option<RecvStream>,
    pub qpack_decoder_recv: Option<RecvStream>,
}

impl H3Connection {
    pub async fn connect(target: &Target) -> Result<Self, ProtocolError> {
        let connection = match create_quic_connection(&target.host, target.port, &target.host).await
        {
            Ok(StreamType::Quic(conn)) => conn,
            Ok(_) => {
                return Err(ProtocolError::RequestFailed(
                    "Expected QUIC connection".to_string(),
                ));
            }
            Err(e) => return Err(ProtocolError::ConnectionFailed(e.to_string())),
        };

        let mut h3_connection = Self::new(connection);
        h3_connection.perform_handshake().await?;
        Ok(h3_connection)
    }

    pub fn new(connection: Connection) -> Self {
        let mut settings = HashMap::new();
        settings.insert(
            SETTINGS_QPACK_MAX_TABLE_CAPACITY,
            DEFAULT_QPACK_MAX_TABLE_CAPACITY,
        );
        settings.insert(
            SETTINGS_MAX_FIELD_SECTION_SIZE,
            DEFAULT_MAX_FIELD_SECTION_SIZE,
        );
        settings.insert(
            SETTINGS_QPACK_BLOCKED_STREAMS,
            DEFAULT_QPACK_BLOCKED_STREAMS,
        );

        let remote_settings = HashMap::new();

        Self {
            connection,
            state: ConnectionState::Idle,
            settings,
            remote_settings,
            streams: HashMap::new(),
            next_stream_id: 0, // Client uses stream IDs 0, 4, 8, 12, ... (bidirectional client-initiated)
            control_send_stream: None,
            control_recv_stream: None,
            qpack_encoder_send: None,
            qpack_decoder_send: None,
            qpack_encoder_recv: None,
            qpack_decoder_recv: None,
        }
    }

    async fn perform_handshake(&mut self) -> Result<(), ProtocolError> {
        // 1. Open control stream (client-initiated unidirectional)
        let send_stream = self.connection.open_uni().await.map_err(|e| {
            ProtocolError::ConnectionFailed(format!("Failed to open control stream: {}", e))
        })?;
        self.control_send_stream = Some(send_stream);

        // 2. Send HTTP/3 stream type on control stream
        // Stream type 0x00 = Control Stream (RFC 9114 Section 6.2.1)
        self.send_stream_type(0x00).await?;

        // 3. Open QPACK unidirectional streams (encoder=0x02, decoder=0x03)
        // Minimal implementation: just send stream types; not used further here.
        if let Ok(enc_send) = self.connection.open_uni().await {
            self.qpack_encoder_send = Some(enc_send);
            Self::send_stream_type_on_option(&mut self.qpack_encoder_send, 0x02).await?;
        }
        if let Ok(dec_send) = self.connection.open_uni().await {
            self.qpack_decoder_send = Some(dec_send);
            Self::send_stream_type_on_option(&mut self.qpack_decoder_send, 0x03).await?;
        }

        // 4. Send initial SETTINGS frame
        let settings_frame = Frame::settings_h3(&[
            (
                SETTINGS_QPACK_MAX_TABLE_CAPACITY,
                self.settings[&SETTINGS_QPACK_MAX_TABLE_CAPACITY],
            ),
            (
                SETTINGS_MAX_FIELD_SECTION_SIZE,
                self.settings[&SETTINGS_MAX_FIELD_SECTION_SIZE],
            ),
            (
                SETTINGS_QPACK_BLOCKED_STREAMS,
                self.settings[&SETTINGS_QPACK_BLOCKED_STREAMS],
            ),
        ]);

        self.send_control_frame(&settings_frame).await?;

        // 5. Accept peer-initiated control stream (unidirectional) and optionally QPACK streams
        // Block until we get a control stream from the peer
        loop {
            let mut recv = self.connection.accept_uni().await.map_err(|e| {
                ProtocolError::ConnectionFailed(format!(
                    "Failed to accept unidirectional stream: {}",
                    e
                ))
            })?;

            // Read stream type varint
            let (stream_type, _) = Self::read_stream_type(&mut recv).await?;
            match stream_type {
                0x00 => {
                    self.control_recv_stream = Some(recv);
                    break;
                }
                0x02 => {
                    self.qpack_encoder_recv = Some(recv);
                }
                0x03 => {
                    self.qpack_decoder_recv = Some(recv);
                }
                _ => {
                    // Ignore unknown uni streams for now (e.g., push streams or extensions)
                }
            }
        }

        // 6. Connection is now open
        self.state = ConnectionState::Open;
        Ok(())
    }

    async fn send_stream_type(&mut self, stream_type: u64) -> Result<(), ProtocolError> {
        if let Some(ref mut send_stream) = self.control_send_stream {
            let mut buf = Vec::new();
            Self::encode_varint_to_vec(&mut buf, stream_type);

            send_stream.write_all(&buf).await.map_err(|e| {
                ProtocolError::ConnectionFailed(format!("Failed to send stream type: {}", e))
            })?;
        } else {
            return Err(ProtocolError::RequestFailed(
                "No control stream available".to_string(),
            ));
        }
        Ok(())
    }

    async fn send_stream_type_on_option(
        stream: &mut Option<SendStream>,
        stream_type: u64,
    ) -> Result<(), ProtocolError> {
        if let Some(send_stream) = stream.as_mut() {
            let mut buf = Vec::new();
            Self::encode_varint_to_vec(&mut buf, stream_type);
            send_stream.write_all(&buf).await.map_err(|e| {
                ProtocolError::ConnectionFailed(format!("Failed to send stream type: {}", e))
            })?;
            Ok(())
        } else {
            Err(ProtocolError::RequestFailed(
                "No stream available".to_string(),
            ))
        }
    }

    async fn read_stream_type(recv_stream: &mut RecvStream) -> Result<(u64, usize), ProtocolError> {
        // Read first byte to determine varint length
        let mut first = [0u8; 1];
        let n = recv_stream
            .read(&mut first)
            .await
            .map_err(|e| {
                ProtocolError::ConnectionFailed(format!("Failed to read stream type prefix: {}", e))
            })?
            .ok_or_else(|| {
                ProtocolError::InvalidResponse("Stream closed before type".to_string())
            })?;
        if n == 0 {
            return Err(ProtocolError::InvalidResponse(
                "Empty stream when reading type".to_string(),
            ));
        }
        let prefix = first[0] >> 6;
        let needed = match prefix {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            _ => 1,
        };
        let mut buf = vec![first[0]];
        if needed > 1 {
            let mut tail = vec![0u8; needed - 1];
            let m = recv_stream
                .read(&mut tail)
                .await
                .map_err(|e| {
                    ProtocolError::ConnectionFailed(format!(
                        "Failed to read stream type bytes: {}",
                        e
                    ))
                })?
                .ok_or_else(|| {
                    ProtocolError::InvalidResponse("Stream closed while reading type".to_string())
                })?;
            if m != needed - 1 {
                return Err(ProtocolError::InvalidResponse(
                    "Incomplete stream type".to_string(),
                ));
            }
            buf.extend_from_slice(&tail);
        }
        let (val, consumed) = Self::decode_varint_from_slice(&buf).ok_or_else(|| {
            ProtocolError::InvalidResponse("Invalid stream type varint".to_string())
        })?;
        Ok((val, consumed))
    }

    async fn send_control_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        if let Some(ref mut send_stream) = self.control_send_stream {
            let serialized = frame.serialize_h3()?;

            send_stream.write_all(&serialized).await.map_err(|e| {
                ProtocolError::ConnectionFailed(format!("Failed to send control frame: {}", e))
            })?;
        } else {
            return Err(ProtocolError::RequestFailed(
                "No control stream available".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn create_request_stream(&mut self) -> Result<(u32, SendStream), ProtocolError> {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 4; // Client uses stream IDs 0, 4, 8, 12, ... for bidirectional streams

        let (send_stream, recv_stream) = self.connection.open_bi().await.map_err(|e| {
            ProtocolError::ConnectionFailed(format!("Failed to open request stream: {}", e))
        })?;

        self.streams.insert(
            stream_id,
            StreamInfo {
                state: StreamState::Open,
                recv_stream,
                recv_buf: BytesMut::new(),
            },
        );

        Ok((stream_id, send_stream))
    }

    pub async fn send_headers(
        &mut self,
        stream_id: u32,
        headers: &[Header],
    ) -> Result<(), ProtocolError> {
        let headers_frame = Frame::headers_h3(stream_id, headers)?;
        self.send_request_frame(stream_id, &headers_frame).await
    }

    pub async fn send_data(&mut self, stream_id: u32, data: &Bytes) -> Result<(), ProtocolError> {
        let data_frame = Frame::data_h3(stream_id, data.clone());
        self.send_request_frame(stream_id, &data_frame).await
    }

    async fn send_request_frame(
        &mut self,
        stream_id: u32,
        frame: &Frame,
    ) -> Result<(), ProtocolError> {
        // Route request frames over the correct bidirectional request stream
        let _ = frame.serialize_h3()?;
        // We don't store the SendStream; callers should write directly, or we can
        // extend this to store SendStream if needed in future.
        Err(ProtocolError::RequestFailed(format!(
            "send_request_frame unavailable: caller must write via SendStream for stream {}",
            stream_id
        )))
    }

    pub async fn read_frame(&mut self) -> Result<Frame, ProtocolError> {
        if let Some(ref mut recv_stream) = self.control_recv_stream {
            let mut buf = [0u8; 8192]; // Buffer for reading control frames

            let n = recv_stream
                .read(&mut buf)
                .await
                .map_err(|e| {
                    ProtocolError::ConnectionFailed(format!(
                        "Failed to read from control stream: {}",
                        e
                    ))
                })?
                .ok_or_else(|| {
                    ProtocolError::InvalidResponse("Control stream closed unexpectedly".to_string())
                })?;

            let (frame, _consumed) = Frame::parse_h3(&buf[..n])?;
            Ok(frame)
        } else {
            Err(ProtocolError::RequestFailed(
                "No control stream available".to_string(),
            ))
        }
    }

    pub async fn read_request_frame(&mut self, stream_id: u32) -> Result<Frame, ProtocolError> {
        let stream_info = self.streams.get_mut(&stream_id).ok_or_else(|| {
            ProtocolError::RequestFailed(format!("Unknown request stream {}", stream_id))
        })?;

        // Maintain a per-stream buffer across reads
        let recv_stream = &mut stream_info.recv_stream;
        let mut local_chunk = vec![0u8; 8192];
        let buf = &mut stream_info.recv_buf;

        loop {
            // Try to parse a frame from current buffer
            if let Some((frame, consumed)) = Self::try_parse_h3_frame(&*buf)? {
                // advance buffer by consumed (drain)
                let _ = buf.split_to(consumed);
                return Ok(frame);
            }

            // Need more data
            let n_opt = recv_stream.read(&mut local_chunk).await.map_err(|e| {
                ProtocolError::ConnectionFailed(format!(
                    "Failed to read from request stream {}: {}",
                    stream_id, e
                ))
            })?;

            match n_opt {
                Some(0) => {
                    // EOF
                    return Err(ProtocolError::InvalidResponse(
                        "Request stream closed".to_string(),
                    ));
                }
                Some(n) => {
                    buf.extend_from_slice(&local_chunk[..n]);
                    continue;
                }
                None => {
                    // FIN with no data
                    return Err(ProtocolError::InvalidResponse(
                        "Request stream closed".to_string(),
                    ));
                }
            }
        }
    }

    fn try_parse_h3_frame(buf: &BytesMut) -> Result<Option<(Frame, usize)>, ProtocolError> {
        if buf.is_empty() {
            return Ok(None);
        }
        // Parse frame type
        let (ftype, t_consumed) = match Self::decode_varint_from_slice(&buf[..]) {
            Some(v) => v,
            None => return Ok(None),
        };
        // Parse length
        if buf.len() <= t_consumed {
            return Ok(None);
        }
        let (len, l_consumed) = match Self::decode_varint_from_slice(&buf[t_consumed..]) {
            Some(v) => v,
            None => return Ok(None),
        };

        let header_len = t_consumed + l_consumed;
        if buf.len() < header_len + (len as usize) {
            return Ok(None);
        }

        let payload = Bytes::copy_from_slice(&buf[header_len..header_len + len as usize]);
        let frame_type = match ftype {
            0x0 => FrameTypeH3::Data,
            0x1 => FrameTypeH3::Headers,
            0x3 => FrameTypeH3::CancelPush,
            0x4 => FrameTypeH3::Settings,
            0x5 => FrameTypeH3::PushPromise,
            0x7 => FrameTypeH3::GoAway,
            0x0d => FrameTypeH3::MaxPushId,
            _ => {
                return Err(ProtocolError::InvalidResponse(format!(
                    "Unknown frame type: {}",
                    ftype
                )));
            }
        };

        let frame = Frame {
            frame_type: FrameType::H3(frame_type),
            flags: 0,
            stream_id: 0,
            payload,
        };
        Ok(Some((frame, header_len + len as usize)))
    }

    pub async fn handle_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        match &frame.frame_type {
            FrameType::H3(FrameTypeH3::Settings) => self.handle_settings_frame(frame).await,
            FrameType::H3(FrameTypeH3::Headers) => self.handle_headers_frame(frame).await,
            FrameType::H3(FrameTypeH3::Data) => self.handle_data_frame(frame).await,
            FrameType::H3(FrameTypeH3::GoAway) => self.handle_goaway_frame(frame).await,
            _ => {
                // Unknown or unhandled frame types are ignored per HTTP/3 spec
                Ok(())
            }
        }
    }

    async fn handle_settings_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        let mut offset = 0;
        while offset + 2 <= frame.payload.len() {
            let (setting_id, consumed) = Self::decode_varint_from_slice(&frame.payload[offset..])
                .ok_or_else(|| {
                ProtocolError::InvalidResponse("Invalid setting ID".to_string())
            })?;
            offset += consumed;

            let (setting_value, consumed) =
                Self::decode_varint_from_slice(&frame.payload[offset..]).ok_or_else(|| {
                    ProtocolError::InvalidResponse("Invalid setting value".to_string())
                })?;
            offset += consumed;

            self.apply_setting(setting_id, setting_value)?;
        }
        Ok(())
    }

    fn apply_setting(&mut self, id: u64, value: u64) -> Result<(), ProtocolError> {
        match id {
            SETTINGS_QPACK_MAX_TABLE_CAPACITY => {
                self.remote_settings.insert(id, value);
            }
            SETTINGS_MAX_FIELD_SECTION_SIZE => {
                self.remote_settings.insert(id, value);
            }
            SETTINGS_QPACK_BLOCKED_STREAMS => {
                self.remote_settings.insert(id, value);
            }
            _ => {
                // Unknown settings are ignored per HTTP/3 spec
            }
        }
        Ok(())
    }

    async fn handle_headers_frame(&mut self, _frame: &Frame) -> Result<(), ProtocolError> {
        // In a real implementation, you'd decode the headers and update stream state
        Ok(())
    }

    async fn handle_data_frame(&mut self, _frame: &Frame) -> Result<(), ProtocolError> {
        // In a real implementation, you'd handle the data and update stream state
        Ok(())
    }

    async fn handle_goaway_frame(&mut self, _frame: &Frame) -> Result<(), ProtocolError> {
        self.state = ConnectionState::Closed;
        Ok(())
    }

    pub async fn send_goaway(&mut self, stream_id: u64) -> Result<(), ProtocolError> {
        let goaway_frame = Frame::goaway_h3(0, stream_id);
        self.send_control_frame(&goaway_frame).await?;
        self.state = ConnectionState::Closed;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        matches!(self.state, ConnectionState::Open)
    }

    pub async fn close(&mut self) -> Result<(), ProtocolError> {
        self.send_goaway(self.next_stream_id as u64).await
    }

    // Helper function to encode varint to Vec<u8>
    fn encode_varint_to_vec(buf: &mut Vec<u8>, value: u64) {
        if value < 0x40 {
            buf.push(value as u8);
        } else if value < 0x4000 {
            let bytes = ((value as u16) | 0x4000).to_be_bytes();
            buf.extend_from_slice(&bytes);
        } else if value < 0x40000000 {
            let bytes = ((value as u32) | 0x80000000).to_be_bytes();
            buf.extend_from_slice(&bytes);
        } else {
            let bytes = (value | 0xC000000000000000).to_be_bytes();
            buf.extend_from_slice(&bytes);
        }
    }

    // Helper function to decode varint from slice
    fn decode_varint_from_slice(data: &[u8]) -> Option<(u64, usize)> {
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
