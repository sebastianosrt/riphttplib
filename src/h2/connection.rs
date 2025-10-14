use crate::h2::framing::FRAME_HEADER_SIZE;
use crate::stream::{create_h2_tls_stream, TransportStream};
use crate::types::{Frame, FrameType, FrameTypeH2, Header, ProtocolError, Target};
use bytes::Bytes;
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// HTTP/2 Connection Preface (RFC 7540 Section 3.5)
const CONNECTION_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

// HTTP/2 Settings Parameters (RFC 7540 Section 6.5.2)
pub const SETTINGS_HEADER_TABLE_SIZE: u16 = 0x1;
pub const SETTINGS_ENABLE_PUSH: u16 = 0x2;
pub const SETTINGS_MAX_CONCURRENT_STREAMS: u16 = 0x3;
pub const SETTINGS_INITIAL_WINDOW_SIZE: u16 = 0x4;
pub const SETTINGS_MAX_FRAME_SIZE: u16 = 0x5;
pub const SETTINGS_MAX_HEADER_LIST_SIZE: u16 = 0x6;

// Default Settings Values
pub const DEFAULT_HEADER_TABLE_SIZE: u32 = 0;
pub const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 100;
pub const DEFAULT_INITIAL_WINDOW_SIZE: u32 = 65535;
pub const DEFAULT_MAX_FRAME_SIZE: u32 = 16384;
pub const DEFAULT_MAX_HEADER_LIST_SIZE: u32 = 8192;

// Connection States
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Idle,
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
}

// Stream States (RFC 7540 Section 5.1) - Client perspective only
#[derive(Debug, Clone, PartialEq)]
pub enum StreamState {
    Idle,
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub state: StreamState,
    pub window_size: i32,
    pub headers_received: bool,
    pub end_stream_received: bool,
    pub end_stream_sent: bool,
}

impl StreamInfo {
    pub fn new(initial_window_size: u32) -> Self {
        Self {
            state: StreamState::Idle,
            window_size: initial_window_size as i32,
            headers_received: false,
            end_stream_received: false,
            end_stream_sent: false,
        }
    }
}

pub struct H2Connection {
    pub stream: TransportStream,
    pub state: ConnectionState,
    pub settings: HashMap<u16, u32>,
    pub remote_settings: HashMap<u16, u32>,
    pub streams: HashMap<u32, StreamInfo>,
    pub connection_window_size: i32,
    pub next_stream_id: u32,
    pub last_stream_id: u32,
}

impl H2Connection {
    pub async fn connect(target: &Target) -> Result<Self, ProtocolError> {
        if target.scheme() != "https" && target.scheme() != "h2" {
            return Err(ProtocolError::RequestFailed(
                "HTTP/2 requires HTTPS".to_string(),
            ));
        }

        let host = target
            .host()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing host".to_string()))?;
        let port = target
            .port()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing port".to_string()))?;

        let stream = create_h2_tls_stream(host, port, host)
            .await
            .map_err(|e| ProtocolError::ConnectionFailed(e.to_string()))?;

        let mut connection = Self::new(stream);
        connection.perform_handshake().await?;
        Ok(connection)
    }

    pub fn new(stream: TransportStream) -> Self {
        let mut settings = HashMap::new();
        settings.insert(SETTINGS_HEADER_TABLE_SIZE, DEFAULT_HEADER_TABLE_SIZE);
        settings.insert(SETTINGS_ENABLE_PUSH, 0);
        settings.insert(
            SETTINGS_MAX_CONCURRENT_STREAMS,
            DEFAULT_MAX_CONCURRENT_STREAMS,
        );
        settings.insert(SETTINGS_INITIAL_WINDOW_SIZE, DEFAULT_INITIAL_WINDOW_SIZE);
        settings.insert(SETTINGS_MAX_FRAME_SIZE, DEFAULT_MAX_FRAME_SIZE);
        settings.insert(SETTINGS_MAX_HEADER_LIST_SIZE, DEFAULT_MAX_HEADER_LIST_SIZE);

        let remote_settings = HashMap::new();

        Self {
            stream,
            state: ConnectionState::Idle,
            settings,
            remote_settings,
            streams: HashMap::new(),
            connection_window_size: DEFAULT_INITIAL_WINDOW_SIZE as i32,
            next_stream_id: 1,
            last_stream_id: 0,
        }
    }

    async fn perform_handshake(&mut self) -> Result<(), ProtocolError> {
        // 1. Send HTTP/2 connection preface
        self.write_to_stream(CONNECTION_PREFACE).await?;

        // 2. Send initial SETTINGS frame
        let settings_frame = Frame::settings(&[
            (
                SETTINGS_HEADER_TABLE_SIZE,
                self.settings[&SETTINGS_HEADER_TABLE_SIZE],
            ),
            (SETTINGS_ENABLE_PUSH, self.settings[&SETTINGS_ENABLE_PUSH]),
            (
                SETTINGS_MAX_CONCURRENT_STREAMS,
                self.settings[&SETTINGS_MAX_CONCURRENT_STREAMS],
            ),
            (
                SETTINGS_INITIAL_WINDOW_SIZE,
                self.settings[&SETTINGS_INITIAL_WINDOW_SIZE],
            ),
            (
                SETTINGS_MAX_FRAME_SIZE,
                self.settings[&SETTINGS_MAX_FRAME_SIZE],
            ),
            (
                SETTINGS_MAX_HEADER_LIST_SIZE,
                self.settings[&SETTINGS_MAX_HEADER_LIST_SIZE],
            ),
        ]);

        self.send_frame(&settings_frame).await?;

        // 3. Connection is now open and ready for frames
        // Remote SETTINGS will be handled asynchronously in handle_frame()
        self.state = ConnectionState::Open;
        Ok(())
    }

    async fn handle_settings_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        if let FrameType::H2(FrameTypeH2::Settings) = &frame.frame_type {
            if frame.is_ack() {
                // Settings ACK - no action needed
                return Ok(());
            }

            // Parse settings payload
            let mut offset = 0;
            while offset + 6 <= frame.payload.len() {
                let id = u16::from_be_bytes([frame.payload[offset], frame.payload[offset + 1]]);
                let value = u32::from_be_bytes([
                    frame.payload[offset + 2],
                    frame.payload[offset + 3],
                    frame.payload[offset + 4],
                    frame.payload[offset + 5],
                ]);

                self.apply_setting(id, value)?;
                offset += 6;
            }

            // Send SETTINGS ACK response
            let settings_ack = Frame::settings_ack();
            self.send_frame(&settings_ack).await?;
        }
        Ok(())
    }

    fn apply_setting(&mut self, id: u16, value: u32) -> Result<(), ProtocolError> {
        match id {
            SETTINGS_HEADER_TABLE_SIZE => {
                self.remote_settings.insert(id, value);
                // Update HPACK encoder table size
            }
            SETTINGS_ENABLE_PUSH => {
                // Client ignores server push settings since we don't support it
                self.remote_settings.insert(id, value);
            }
            SETTINGS_MAX_CONCURRENT_STREAMS => {
                self.remote_settings.insert(id, value);
            }
            SETTINGS_INITIAL_WINDOW_SIZE => {
                if value > 0x7FFFFFFF {
                    return Err(ProtocolError::InvalidResponse(
                        "Invalid INITIAL_WINDOW_SIZE value".to_string(),
                    ));
                }
                let old_value = self
                    .remote_settings
                    .get(&id)
                    .unwrap_or(&DEFAULT_INITIAL_WINDOW_SIZE);
                let delta = value as i32 - *old_value as i32;

                // Update all stream window sizes
                for stream in self.streams.values_mut() {
                    stream.window_size += delta;
                }

                self.remote_settings.insert(id, value);
            }
            SETTINGS_MAX_FRAME_SIZE => {
                if value < 16384 || value > 16777215 {
                    return Err(ProtocolError::InvalidResponse(
                        "Invalid MAX_FRAME_SIZE value".to_string(),
                    ));
                }
                self.remote_settings.insert(id, value);
            }
            SETTINGS_MAX_HEADER_LIST_SIZE => {
                self.remote_settings.insert(id, value);
            }
            _ => {
                // Unknown settings are ignored per RFC 7540
            }
        }
        Ok(())
    }

    pub async fn create_stream(&mut self) -> Result<u32, ProtocolError> {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 2; // Client uses odd numbers (1, 3, 5, ...)

        let initial_window_size = self
            .remote_settings
            .get(&SETTINGS_INITIAL_WINDOW_SIZE)
            .unwrap_or(&DEFAULT_INITIAL_WINDOW_SIZE);

        let stream_info = StreamInfo::new(*initial_window_size);
        self.streams.insert(stream_id, stream_info);

        Ok(stream_id)
    }

    pub fn get_stream_state(&self, stream_id: u32) -> Option<&StreamState> {
        self.streams.get(&stream_id).map(|s| &s.state)
    }

    pub fn update_stream_state(
        &mut self,
        stream_id: u32,
        new_state: StreamState,
    ) -> Result<(), ProtocolError> {
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.state = new_state;
            Ok(())
        } else {
            Err(ProtocolError::RequestFailed(format!(
                "Stream {} not found",
                stream_id
            )))
        }
    }

    pub async fn send_headers(
        &mut self,
        stream_id: u32,
        headers: &[Header],
        end_stream: bool,
    ) -> Result<(), ProtocolError> {
        let end_headers = true; // For simplicity, assume headers fit in one frame
        let headers_frame = Frame::headers(stream_id, headers, end_stream, end_headers)?;

        self.send_frame(&headers_frame).await?;

        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.headers_received = true;
            if end_stream {
                stream.end_stream_sent = true;
                stream.state = StreamState::HalfClosedLocal;
            } else {
                stream.state = StreamState::Open;
            }
        }

        Ok(())
    }

    pub async fn send_data(
        &mut self,
        stream_id: u32,
        data: &[u8],
        end_stream: bool,
    ) -> Result<(), ProtocolError> {
        // Check flow control
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            if stream.window_size < data.len() as i32 {
                return Err(ProtocolError::RequestFailed(
                    "Stream flow control window exceeded".to_string(),
                ));
            }
            stream.window_size -= data.len() as i32;
        }

        if self.connection_window_size < data.len() as i32 {
            return Err(ProtocolError::RequestFailed(
                "Connection flow control window exceeded".to_string(),
            ));
        }
        self.connection_window_size -= data.len() as i32;

        let data_frame = Frame::data(stream_id, Bytes::copy_from_slice(data), end_stream);
        self.send_frame(&data_frame).await?;

        if end_stream {
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                stream.end_stream_sent = true;
                stream.state = match stream.state {
                    StreamState::Open => StreamState::HalfClosedLocal,
                    StreamState::HalfClosedRemote => StreamState::Closed,
                    _ => stream.state.clone(),
                };
            }
        }

        Ok(())
    }

    pub async fn send_window_update(
        &mut self,
        stream_id: u32,
        increment: u32,
    ) -> Result<(), ProtocolError> {
        let window_update_frame = Frame::window_update(stream_id, increment)?;
        self.send_frame(&window_update_frame).await?;

        if stream_id == 0 {
            // Connection-level window update
            self.connection_window_size += increment as i32;
        } else if let Some(stream) = self.streams.get_mut(&stream_id) {
            // Stream-level window update
            stream.window_size += increment as i32;
        }

        Ok(())
    }

    pub async fn send_rst(&mut self, stream_id: u32, error_code: u32) -> Result<(), ProtocolError> {
        let rst_frame = Frame::rst(stream_id, error_code);
        self.send_frame(&rst_frame).await?;

        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.state = StreamState::Closed;
        }

        Ok(())
    }

    pub async fn send_ping(&mut self, data: [u8; 8]) -> Result<(), ProtocolError> {
        let ping_frame = Frame::ping(data);
        self.send_frame(&ping_frame).await
    }

    pub async fn send_ping_ack(&mut self, data: [u8; 8]) -> Result<(), ProtocolError> {
        let ping_ack_frame = Frame::ping_ack(data);
        self.send_frame(&ping_ack_frame).await
    }

    pub async fn send_goaway(
        &mut self,
        last_stream_id: u32,
        error_code: u32,
        debug_data: Option<&[u8]>,
    ) -> Result<(), ProtocolError> {
        let goaway_frame = Frame::goaway(last_stream_id, error_code, debug_data);
        self.send_frame(&goaway_frame).await?;

        self.state = ConnectionState::Closed;
        Ok(())
    }

    pub async fn handle_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        match &frame.frame_type {
            FrameType::H2(FrameTypeH2::Headers) => self.handle_headers_frame(frame).await,
            FrameType::H2(FrameTypeH2::Data) => self.handle_data_frame(frame).await,
            FrameType::H2(FrameTypeH2::Settings) => self.handle_settings_frame(frame).await,
            FrameType::H2(FrameTypeH2::WindowUpdate) => {
                self.handle_window_update_frame(frame).await
            }
            FrameType::H2(FrameTypeH2::RstStream) => self.handle_rst_stream_frame(frame).await,
            FrameType::H2(FrameTypeH2::Ping) => self.handle_ping_frame(frame).await,
            FrameType::H2(FrameTypeH2::GoAway) => self.handle_goaway_frame(frame).await,
            _ => Ok(()),
        }
    }

    async fn handle_headers_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        let stream_id = frame.stream_id;

        // Create stream if it doesn't exist
        if !self.streams.contains_key(&stream_id) {
            let initial_window_size = self
                .settings
                .get(&SETTINGS_INITIAL_WINDOW_SIZE)
                .unwrap_or(&DEFAULT_INITIAL_WINDOW_SIZE);
            let stream_info = StreamInfo::new(*initial_window_size);
            self.streams.insert(stream_id, stream_info);
        }

        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.headers_received = true;

            if frame.is_end_stream() {
                stream.end_stream_received = true;
                stream.state = match stream.state {
                    StreamState::Idle => StreamState::HalfClosedRemote,
                    StreamState::Open => StreamState::HalfClosedRemote,
                    StreamState::HalfClosedLocal => StreamState::Closed,
                    _ => stream.state.clone(),
                };
            } else {
                stream.state = StreamState::Open;
            }
        }

        Ok(())
    }

    // TODO: rework
    async fn handle_data_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        let stream_id = frame.stream_id;
        let data_size = frame.payload.len() as u32;

        // Update flow control windows
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.window_size -= data_size as i32;
        }
        self.connection_window_size -= data_size as i32;

        // Send WINDOW_UPDATE if needed (simple policy: update when window gets below 50%)
        let initial_window_size = self
            .settings
            .get(&SETTINGS_INITIAL_WINDOW_SIZE)
            .unwrap_or(&DEFAULT_INITIAL_WINDOW_SIZE);

        if let Some(stream) = self.streams.get(&stream_id) {
            if stream.window_size < (*initial_window_size as i32) / 2 {
                let increment = *initial_window_size - stream.window_size as u32;
                self.send_window_update(stream_id, increment).await?;
            }
        }

        if self.connection_window_size < (DEFAULT_INITIAL_WINDOW_SIZE as i32) / 2 {
            let increment = DEFAULT_INITIAL_WINDOW_SIZE - self.connection_window_size as u32;
            self.send_window_update(0, increment).await?;
        }

        if frame.is_end_stream() {
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                stream.end_stream_received = true;
                stream.state = match stream.state {
                    StreamState::Open => StreamState::HalfClosedRemote,
                    StreamState::HalfClosedLocal => StreamState::Closed,
                    _ => stream.state.clone(),
                };
            }
        }

        Ok(())
    }

    async fn handle_window_update_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        if frame.payload.len() != 4 {
            return Err(ProtocolError::InvalidResponse(
                "Invalid WINDOW_UPDATE frame size".to_string(),
            ));
        }

        let increment = u32::from_be_bytes([
            frame.payload[0],
            frame.payload[1],
            frame.payload[2],
            frame.payload[3],
        ]) & 0x7FFFFFFF;

        if frame.stream_id == 0 {
            // Connection-level window update
            self.connection_window_size += increment as i32;
        } else if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
            // Stream-level window update
            stream.window_size += increment as i32;
        }

        Ok(())
    }

    async fn handle_rst_stream_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
            stream.state = StreamState::Closed;
        }
        Ok(())
    }

    async fn handle_ping_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        if !frame.is_ack() {
            // Send PING ACK with same data
            if frame.payload.len() == 8 {
                let mut data = [0u8; 8];
                data.copy_from_slice(&frame.payload);
                self.send_ping_ack(data).await?;
            }
        }
        Ok(())
    }

    async fn handle_goaway_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        if frame.payload.len() < 8 {
            return Err(ProtocolError::InvalidResponse(
                "Invalid GOAWAY frame size".to_string(),
            ));
        }

        let last_stream_id = u32::from_be_bytes([
            frame.payload[0],
            frame.payload[1],
            frame.payload[2],
            frame.payload[3],
        ]) & 0x7FFFFFFF;

        let _error_code = u32::from_be_bytes([
            frame.payload[4],
            frame.payload[5],
            frame.payload[6],
            frame.payload[7],
        ]);

        self.last_stream_id = last_stream_id;
        self.state = ConnectionState::Closed;

        Ok(())
    }

    pub async fn send_frame(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        let serialized = frame.serialize()?;
        self.write_to_stream(&serialized).await
    }

    pub async fn read_frame(&mut self) -> Result<Frame, ProtocolError> {
        // Read frame header (9 bytes)
        let mut header_buf = [0u8; FRAME_HEADER_SIZE];
        self.read_from_stream(&mut header_buf).await?;

        // Parse header to get payload length
        let length =
            ((header_buf[0] as u32) << 16) | ((header_buf[1] as u32) << 8) | (header_buf[2] as u32);

        // Read payload
        let mut payload_buf = vec![0u8; length as usize];
        if length > 0 {
            self.read_from_stream(&mut payload_buf).await?;
        }

        // Combine header and payload for parsing
        let mut frame_buf = Vec::with_capacity(FRAME_HEADER_SIZE + length as usize);
        frame_buf.extend_from_slice(&header_buf);
        frame_buf.extend_from_slice(&payload_buf);

        Frame::parse(&frame_buf)
    }

    async fn write_to_stream(&mut self, data: &[u8]) -> Result<(), ProtocolError> {
        match &mut self.stream {
            TransportStream::Tcp(tcp) => tcp.write_all(data).await.map_err(ProtocolError::Io)?,
            TransportStream::Tls(tls) => tls.write_all(data).await.map_err(ProtocolError::Io)?,
        }
        Ok(())
    }

    async fn read_from_stream(&mut self, buffer: &mut [u8]) -> Result<usize, ProtocolError> {
        match &mut self.stream {
            TransportStream::Tcp(tcp) => tcp
                .read_exact(buffer)
                .await
                .map_err(ProtocolError::Io)
                .map(|_| buffer.len()),
            TransportStream::Tls(tls) => tls
                .read_exact(buffer)
                .await
                .map_err(ProtocolError::Io)
                .map(|_| buffer.len()),
        }
    }

    pub fn is_connection_open(&self) -> bool {
        matches!(self.state, ConnectionState::Open)
    }

    pub fn get_max_concurrent_streams(&self) -> u32 {
        self.remote_settings
            .get(&SETTINGS_MAX_CONCURRENT_STREAMS)
            .unwrap_or(&DEFAULT_MAX_CONCURRENT_STREAMS)
            .clone()
    }

    pub fn get_active_stream_count(&self) -> usize {
        self.streams
            .values()
            .filter(|s| {
                matches!(
                    s.state,
                    StreamState::Open
                        | StreamState::HalfClosedLocal
                        | StreamState::HalfClosedRemote
                )
            })
            .count()
    }

    pub async fn close(&mut self) -> Result<(), ProtocolError> {
        self.send_goaway(self.last_stream_id, 0, None).await
    }

    pub fn generate_stream_ids(n: i32) -> Vec<u32> {
        let mut stream_ids = Vec::with_capacity(n as usize);
        let mut current_id = 1u32;

        for _ in 0..n {
            stream_ids.push(current_id);
            current_id += 2;
        }

        stream_ids
    }
}
