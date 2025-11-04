mod state;

pub use state::{ConnectionState, StreamEvent, StreamInfo, StreamState};

use crate::connection::HttpConnection;
use crate::h2::consts::*;
use crate::h2::framing::RstErrorCode;
use crate::h2::hpack::HpackCodec;
use crate::stream::{create_stream, TransportStream};
use crate::types::{
    ClientTimeouts, FrameH2, FrameSink, FrameType, FrameTypeH2, H2ConnectionErrorKind, H2ErrorCode,
    H2StreamErrorKind, Header, ProtocolError, ResponseFrame,
};
use crate::utils::timeout_result;
use crate::Response;
use async_trait::async_trait;
use bytes::Bytes;
use state::PendingHeaderBlock;
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct H2Connection {
    pub stream: TransportStream,
    pub state: ConnectionState,
    pub settings: HashMap<u16, u32>,
    pub remote_settings: HashMap<u16, u32>,
    pub streams: HashMap<u32, StreamInfo>,
    pub send_connection_window: i32,
    pub recv_connection_window: i32,
    pub next_stream_id: u32,
    pub last_stream_id: u32,
    hpack: HpackCodec,
    initial_settings_received: bool,
    peer_allows_push: bool,
    goaway_reason: Option<(H2ErrorCode, String)>,
    goaway_last_stream_id: Option<u32>,
    goaway_received: bool,
    pending_writes: Vec<Bytes>,
    pending_write_bytes: usize,
    auto_flush_bytes: Option<usize>,
    timeouts: ClientTimeouts,
    captured_frames: HashMap<u32, Vec<FrameH2>>,
}

#[derive(Debug, Clone)]
pub struct H2ConnectOptions {
    pub target: String,
    pub timeouts: ClientTimeouts,
}

impl H2Connection {
    pub async fn connect(
        target: &str,
        timeouts: &ClientTimeouts, // TODO make optional
    ) -> Result<Self, ProtocolError> {
        let target = crate::utils::parse_target(target)?;
        let scheme = target.scheme();
        let is_tls = scheme == "https";
        let is_h2c = scheme == "http";

        if !is_tls && !is_h2c {
            return Err(ProtocolError::RequestFailed(
                "HTTP/2 requires https, h2, h2c, or http schemes".to_string(),
            ));
        }

        let host = target
            .host()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing host".to_string()))?;
        let port = target
            .port()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing port".to_string()))?;

        let transport = if is_tls {
            create_stream("h2", host, port, timeouts.connect)
                .await
                .map_err(|e| ProtocolError::ConnectionFailed(e.to_string()))?
        } else {
            create_stream("http", host, port, timeouts.connect)
                .await
                .map_err(|e| ProtocolError::ConnectionFailed(e.to_string()))?
        };

        let mut connection = Self::new(transport, timeouts.clone());
        connection.perform_handshake().await?;
        Ok(connection)
    }

    pub fn new(stream: TransportStream, timeouts: ClientTimeouts) -> Self {
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
        let local_table_size = settings
            .get(&SETTINGS_HEADER_TABLE_SIZE)
            .copied()
            .unwrap_or(DEFAULT_HEADER_TABLE_SIZE) as usize;
        let hpack = HpackCodec::new(
            local_table_size,
            DEFAULT_HEADER_TABLE_SIZE.max(4096) as usize,
        );

        Self {
            stream,
            state: ConnectionState::Idle,
            settings,
            remote_settings,
            streams: HashMap::new(),
            send_connection_window: DEFAULT_INITIAL_WINDOW_SIZE as i32,
            recv_connection_window: DEFAULT_INITIAL_WINDOW_SIZE as i32,
            next_stream_id: 1,
            last_stream_id: 0,
            hpack,
            initial_settings_received: false,
            peer_allows_push: true,
            goaway_reason: None,
            goaway_last_stream_id: None,
            goaway_received: false,
            pending_writes: Vec::new(),
            pending_write_bytes: 0,
            auto_flush_bytes: None,
            timeouts,
            captured_frames: HashMap::new(),
        }
    }

    async fn perform_handshake(&mut self) -> Result<(), ProtocolError> {
        // 1. Send HTTP/2 connection preface
        self.write_to_stream(CONNECTION_PREFACE).await?;

        // 2. Send initial SETTINGS frame
        FrameH2::settings(&[
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
        ])
        .send(self)
        .await?;

        self.flush().await?;

        // 3. Await the peer's initial SETTINGS frame before proceeding.
        self.await_initial_settings().await?;

        // 3. Connection is now open and ready for frames
        // Remote SETTINGS will be handled asynchronously in handle_frame()
        self.state = ConnectionState::Open;
        Ok(())
    }

    async fn await_initial_settings(&mut self) -> Result<(), ProtocolError> {
        while !self.initial_settings_received {
            let frame = self.read_frame_from_wire().await?;
            match frame.frame_type {
                FrameType::H2(FrameTypeH2::Settings) => {
                    let is_ack = frame.is_ack();
                    self.handle_settings_frame(&frame).await?;
                    if !is_ack {
                        self.initial_settings_received = true;
                    }
                }
                _ => {
                    self.process_incoming_frame(frame).await?;
                }
            }
        }
        Ok(())
    }

    async fn handle_settings_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
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
            FrameH2::settings_ack().send(self).await?;
        }
        Ok(())
    }

    fn apply_setting(&mut self, id: u16, value: u32) -> Result<(), ProtocolError> {
        match id {
            SETTINGS_HEADER_TABLE_SIZE => {
                self.remote_settings.insert(id, value);
                self.hpack.set_encoder_max_table_size(value as usize);
            }
            SETTINGS_ENABLE_PUSH => {
                self.remote_settings.insert(id, value);
                self.peer_allows_push = value != 0;
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
                    stream.send_window = (stream.send_window + delta).clamp(0, 0x7FFF_FFFF);
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
        if !self.initial_settings_received {
            return Err(ProtocolError::RequestFailed(
                "HTTP/2 handshake not complete".to_string(),
            ));
        }
        if !self.is_connection_open() {
            return Err(ProtocolError::ConnectionFailed(
                "HTTP/2 connection is not open".to_string(),
            ));
        }

        if let Some(last) = self.goaway_last_stream_id {
            if self.next_stream_id > last {
                return Err(ProtocolError::RequestFailed(
                    "GOAWAY received: new streams are not allowed".to_string(),
                ));
            }
        }

        let stream_id = self.next_stream_id;
        self.next_stream_id += 2;

        let send_window = self.peer_initial_stream_window();
        let recv_window = self.local_initial_stream_window();

        let stream_info = StreamInfo::new(send_window, recv_window);
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
        let frames = self.encode_headers_frames(stream_id, headers, end_stream)?;
        for frame in frames {
            frame.send(self).await?;
        }

        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.headers_sent = true;
            if end_stream {
                stream.end_stream_sent = true;
                stream.state = StreamState::HalfClosedLocal;
            } else {
                stream.state = StreamState::Open;
            }
        }

        Ok(())
    }

    pub fn build_headers_frames(
        &mut self,
        stream_id: u32,
        headers: &[Header],
        end_stream: bool,
    ) -> Result<Vec<FrameH2>, ProtocolError> {
        self.encode_headers_frames(stream_id, headers, end_stream)
    }

    fn encode_headers_frames(
        &mut self,
        stream_id: u32,
        headers: &[Header],
        end_stream: bool,
    ) -> Result<Vec<FrameH2>, ProtocolError> {
        let mut encoded = self.hpack.encode(headers)?;
        let max_frame = self.max_frame_size();
        let mut first = true;
        let mut frames = Vec::new();

        loop {
            let chunk_len = encoded.len().min(max_frame);
            let chunk = if chunk_len > 0 {
                encoded.split_to(chunk_len)
            } else {
                Bytes::new()
            };

            let is_last = encoded.is_empty();
            let mut flags = 0u8;

            if first && end_stream {
                flags |= END_STREAM_FLAG;
            }
            if is_last {
                flags |= END_HEADERS_FLAG;
            }

            let frame_type = if first {
                FrameTypeH2::Headers
            } else {
                FrameTypeH2::Continuation
            };

            frames.push(FrameH2::new(frame_type, flags, stream_id, chunk));

            if is_last {
                break;
            }
            first = false;
        }

        Ok(frames)
    }

    pub async fn send_data(
        &mut self,
        stream_id: u32,
        data: &[u8],
        end_stream: bool,
    ) -> Result<(), ProtocolError> {
        let data_len = data.len();

        if data_len == 0 && !end_stream {
            return Ok(());
        }

        if data_len > self.max_frame_size() {
            return Err(ProtocolError::RequestFailed(
                "DATA frame exceeds peer advertised MAX_FRAME_SIZE".to_string(),
            ));
        }

        // Check flow control
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            if stream.send_window < data_len as i32 {
                return Err(ProtocolError::H2FlowControlError(
                    "Stream flow control window exceeded".to_string(),
                ));
            }
            stream.send_window -= data_len as i32;
        } else {
            return Err(ProtocolError::RequestFailed(format!(
                "Stream {} not found",
                stream_id
            )));
        }

        if self.send_connection_window < data_len as i32 {
            return Err(ProtocolError::H2FlowControlError(
                "Connection flow control window exceeded".to_string(),
            ));
        }
        self.send_connection_window -= data_len as i32;

        FrameH2::data(stream_id, Bytes::copy_from_slice(data), end_stream)
            .send(self)
            .await?;

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
        if increment == 0 {
            return Err(ProtocolError::H2ProtocolError(
                "WINDOW_UPDATE increment must be greater than zero".to_string(),
            ));
        }
        FrameH2::window_update(stream_id, increment)?
            .send(self)
            .await?;

        if stream_id == 0 {
            // Connection-level window update
            let new_window = self
                .recv_connection_window
                .saturating_add(Self::clamp_window(increment));
            self.recv_connection_window = new_window;
        } else if let Some(stream) = self.streams.get_mut(&stream_id) {
            // Stream-level window update
            let new_window = stream
                .recv_window
                .saturating_add(Self::clamp_window(increment));
            stream.recv_window = new_window;
        }

        Ok(())
    }

    pub async fn send_rst(
        &mut self,
        stream_id: u32,
        error_code: RstErrorCode,
    ) -> Result<(), ProtocolError> {
        FrameH2::rst(stream_id, error_code.into())
            .send(self)
            .await?;

        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.state = StreamState::Closed;
        }

        Ok(())
    }

    pub async fn send_goaway(
        &mut self,
        last_stream_id: u32,
        error_code: u32,
        debug_data: Option<&[u8]>,
    ) -> Result<(), ProtocolError> {
        FrameH2::goaway(last_stream_id, error_code, debug_data)
            .send(self)
            .await?;

        self.state = ConnectionState::Closed;
        Ok(())
    }

    pub async fn handle_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
        self.process_incoming_frame(frame.clone()).await
    }

    async fn handle_headers_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
        if frame.stream_id == 0 {
            return Err(ProtocolError::H2ProtocolError(
                "HEADERS frame received on stream 0".to_string(),
            ));
        }

        let stream_id = frame.stream_id;
        self.ensure_stream(stream_id);

        if frame.is_end_stream() {
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                stream.end_stream_received = true;
                stream.state = match stream.state {
                    StreamState::Idle | StreamState::Open => StreamState::HalfClosedRemote,
                    StreamState::HalfClosedLocal => StreamState::Closed,
                    StreamState::HalfClosedRemote | StreamState::Closed => stream.state.clone(),
                };
            }
        } else {
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                if matches!(stream.state, StreamState::Idle) {
                    stream.state = StreamState::Open;
                }
            }
        }

        Ok(())
    }

    // TODO: rework
    async fn handle_data_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
        let stream_id = frame.stream_id;
        if stream_id == 0 {
            return Err(ProtocolError::H2ProtocolError(
                "DATA frame received on stream 0".to_string(),
            ));
        }

        self.ensure_stream(stream_id);

        let data_size = frame.payload.len() as u32;
        if data_size == 0 {
            return Ok(());
        }

        let data_window = Self::clamp_window(data_size);

        // Update flow control windows
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            if stream.recv_window < data_window {
                return Err(ProtocolError::H2FlowControlError(
                    "Peer violated stream flow control".to_string(),
                ));
            }
            stream.recv_window -= data_window;
        } else {
            return Err(ProtocolError::RequestFailed(format!(
                "Stream {} not found",
                stream_id
            )));
        }

        if self.recv_connection_window < data_window {
            return Err(ProtocolError::H2FlowControlError(
                "Peer violated connection flow control".to_string(),
            ));
        }
        self.recv_connection_window -= data_window;

        // Release flow control credit now that the payload has been consumed.
        self.send_window_update(stream_id, data_size).await?;
        self.send_window_update(0, data_size).await?;

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

    async fn handle_window_update_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
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

        if increment == 0 {
            return Err(ProtocolError::H2ProtocolError(
                "WINDOW_UPDATE increment must be greater than zero".to_string(),
            ));
        }

        let increment_val = Self::clamp_window(increment);

        if frame.stream_id == 0 {
            // Connection-level window update
            let new_window = self
                .send_connection_window
                .saturating_add(increment_val)
                .clamp(0, 0x7FFF_FFFF);
            self.send_connection_window = new_window;
        } else if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
            // Stream-level window update
            let new_window = stream
                .send_window
                .saturating_add(increment_val)
                .clamp(0, 0x7FFF_FFFF);
            stream.send_window = new_window;
        }

        Ok(())
    }

    async fn handle_rst_stream_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
        if frame.payload.len() != 4 {
            return Err(ProtocolError::H2ProtocolError(
                "RST_STREAM frame must have 4-byte payload".to_string(),
            ));
        }

        let error_code = u32::from_be_bytes([
            frame.payload[0],
            frame.payload[1],
            frame.payload[2],
            frame.payload[3],
        ]);

        if let Some(stream) = self.streams.get_mut(&frame.stream_id) {
            stream.state = StreamState::Closed;
        }

        let h2_error = H2ErrorCode::from(error_code);
        Err(ProtocolError::H2StreamError(H2StreamErrorKind::Reset(
            h2_error,
        )))
    }

    async fn handle_ping_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
        if !frame.is_ack() {
            // Send PING ACK with same data
            if frame.payload.len() == 8 {
                let mut data = [0u8; 8];
                data.copy_from_slice(&frame.payload);
                let _ = FrameH2::ping_ack(data).send(self).await;
            }
        }
        Ok(())
    }

    async fn handle_goaway_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
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

        let error_code = u32::from_be_bytes([
            frame.payload[4],
            frame.payload[5],
            frame.payload[6],
            frame.payload[7],
        ]);
        let h2_error = H2ErrorCode::from(error_code);

        let debug_data = if frame.payload.len() > 8 {
            String::from_utf8_lossy(&frame.payload[8..]).to_string()
        } else {
            String::new()
        };

        self.last_stream_id = last_stream_id;
        self.goaway_last_stream_id = Some(last_stream_id);
        self.goaway_reason = Some((h2_error, debug_data.clone()));
        self.goaway_received = true;
        if !matches!(
            self.state,
            ConnectionState::Closed | ConnectionState::HalfClosedRemote
        ) {
            self.state = ConnectionState::HalfClosedRemote;
        }

        for (&id, stream) in self.streams.iter_mut() {
            if id > last_stream_id {
                stream.state = StreamState::Closed;
            }
        }

        Err(ProtocolError::H2ConnectionError(
            H2ConnectionErrorKind::GoAway(h2_error, debug_data),
        ))
    }

    pub async fn recv_stream_event(
        &mut self,
        stream_id: u32,
    ) -> Result<StreamEvent, ProtocolError> {
        if stream_id == 0 {
            return Err(ProtocolError::H2ProtocolError(
                "Cannot receive events for stream 0".to_string(),
            ));
        }

        self.ensure_stream(stream_id);

        loop {
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                if let Some(event) = stream.inbound_events.pop_front() {
                    return Ok(event);
                }
            }

            if matches!(self.state, ConnectionState::Closed) {
                return Err(self.goaway_error());
            }

            match self.pump_incoming().await {
                Ok(()) => {}
                Err(err) => return Err(err),
            }
        }
    }

    async fn pump_incoming(&mut self) -> Result<(), ProtocolError> {
        let frame = self.read_frame_from_wire().await?;
        self.process_incoming_frame(frame).await
    }

    async fn process_incoming_frame(&mut self, frame: FrameH2) -> Result<(), ProtocolError> {
        self.record_frame(&frame);
        match &frame.frame_type {
            FrameType::H2(FrameTypeH2::Headers) => {
                self.handle_headers_frame(&frame).await?;
                if let Some(event) = self.handle_header_block_fragment(&frame)? {
                    self.enqueue_stream_event(frame.stream_id, event);
                }
            }
            FrameType::H2(FrameTypeH2::Continuation) => {
                if let Some(event) = self.handle_header_block_fragment(&frame)? {
                    self.enqueue_stream_event(frame.stream_id, event);
                }
            }
            FrameType::H2(FrameTypeH2::Data) => {
                self.handle_data_frame(&frame).await?;
                let payload = Self::data_payload(&frame)?;
                let end_stream = frame.is_end_stream();
                self.enqueue_stream_event(
                    frame.stream_id,
                    StreamEvent::Data {
                        payload,
                        end_stream,
                    },
                );
            }
            FrameType::H2(FrameTypeH2::RstStream) => {
                match self.handle_rst_stream_frame(&frame).await {
                    Ok(_) => {}
                    Err(err) => {
                        if let ProtocolError::H2StreamError(H2StreamErrorKind::Reset(code)) = err {
                            self.enqueue_stream_event(
                                frame.stream_id,
                                StreamEvent::RstStream { error_code: code },
                            );
                        } else {
                            return Err(err);
                        }
                    }
                }
            }
            FrameType::H2(FrameTypeH2::Settings) => {
                self.handle_settings_frame(&frame).await?;
            }
            FrameType::H2(FrameTypeH2::WindowUpdate) => {
                self.handle_window_update_frame(&frame).await?;
            }
            FrameType::H2(FrameTypeH2::Ping) => {
                self.handle_ping_frame(&frame).await?;
            }
            FrameType::H2(FrameTypeH2::PushPromise) => {
                if !self.peer_allows_push {
                    return Err(ProtocolError::H2ProtocolError(
                        "PUSH_PROMISE received but push is disabled".to_string(),
                    ));
                } else {
                    return Err(ProtocolError::H2ProtocolError(
                        "PUSH_PROMISE handling is not implemented".to_string(),
                    ));
                }
            }
            FrameType::H2(FrameTypeH2::GoAway) => {
                return self.handle_goaway_frame(&frame).await;
            }
            _ => { /* Ignore unsupported frame types */ }
        }

        Ok(())
    }

    fn ensure_stream(&mut self, stream_id: u32) {
        if !self.streams.contains_key(&stream_id) {
            let send_window = self.peer_initial_stream_window();
            let recv_window = self.local_initial_stream_window();
            self.streams
                .insert(stream_id, StreamInfo::new(send_window, recv_window));
        }
    }

    fn max_frame_size(&self) -> usize {
        self.remote_settings
            .get(&SETTINGS_MAX_FRAME_SIZE)
            .copied()
            .unwrap_or(DEFAULT_MAX_FRAME_SIZE) as usize
    }

    fn peer_initial_stream_window(&self) -> i32 {
        Self::clamp_window(
            self.remote_settings
                .get(&SETTINGS_INITIAL_WINDOW_SIZE)
                .copied()
                .unwrap_or(DEFAULT_INITIAL_WINDOW_SIZE),
        )
    }

    fn local_initial_stream_window(&self) -> i32 {
        Self::clamp_window(
            self.settings
                .get(&SETTINGS_INITIAL_WINDOW_SIZE)
                .copied()
                .unwrap_or(DEFAULT_INITIAL_WINDOW_SIZE),
        )
    }

    fn clamp_window(value: u32) -> i32 {
        let capped = value.min(0x7FFF_FFFF);
        capped as i32
    }

    fn goaway_error(&self) -> ProtocolError {
        if let Some((code, debug)) = &self.goaway_reason {
            ProtocolError::H2ConnectionError(H2ConnectionErrorKind::GoAway(*code, debug.clone()))
        } else {
            ProtocolError::ConnectionFailed("HTTP/2 connection closed".to_string())
        }
    }

    pub fn set_auto_flush_bytes(&mut self, threshold: Option<usize>) {
        self.auto_flush_bytes = threshold;
    }

    pub async fn flush(&mut self) -> Result<(), ProtocolError> {
        self.flush_pending_writes().await
    }

    async fn flush_pending_writes(&mut self) -> Result<(), ProtocolError> {
        if self.pending_writes.is_empty() {
            return Ok(());
        }

        let mut aggregate = Vec::with_capacity(self.pending_write_bytes);
        for chunk in self.pending_writes.drain(..) {
            aggregate.extend_from_slice(&chunk);
        }
        self.pending_write_bytes = 0;

        self.write_to_stream(&aggregate).await
    }

    fn enqueue_stream_event(&mut self, stream_id: u32, event: StreamEvent) {
        if stream_id == 0 {
            return;
        }
        self.ensure_stream(stream_id);
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.inbound_events.push_back(event);
        }
    }

    fn handle_header_block_fragment(
        &mut self,
        frame: &FrameH2,
    ) -> Result<Option<StreamEvent>, ProtocolError> {
        let stream_id = frame.stream_id;
        if stream_id == 0 {
            return Err(ProtocolError::H2ProtocolError(
                "Header block on stream 0".to_string(),
            ));
        }

        self.ensure_stream(stream_id);

        match frame.frame_type {
            FrameType::H2(FrameTypeH2::Headers) => {
                let fragment = self.header_fragment_bytes(frame)?;
                let end_stream = frame.is_end_stream();
                if frame.is_end_headers() {
                    let event =
                        self.decode_header_block(stream_id, fragment.as_ref(), end_stream)?;
                    Ok(Some(event))
                } else {
                    let mut pending = PendingHeaderBlock::new();
                    pending.end_stream = end_stream;
                    pending.append(fragment.as_ref());
                    if let Some(stream) = self.streams.get_mut(&stream_id) {
                        stream.pending_headers = Some(pending);
                    }
                    Ok(None)
                }
            }
            FrameType::H2(FrameTypeH2::Continuation) => {
                if (frame.flags & PADDED_FLAG) != 0 {
                    return Err(ProtocolError::H2ProtocolError(
                        "CONTINUATION frame must not be padded".to_string(),
                    ));
                }

                let fragment = frame.payload.clone();
                {
                    let stream = self.streams.get_mut(&stream_id).ok_or_else(|| {
                        ProtocolError::H2ProtocolError(
                            "CONTINUATION frame without a preceding HEADERS frame".to_string(),
                        )
                    })?;
                    if let Some(pending) = stream.pending_headers.as_mut() {
                        pending.append(fragment.as_ref());
                    } else {
                        return Err(ProtocolError::H2ProtocolError(
                            "CONTINUATION frame without pending header block".to_string(),
                        ));
                    }
                }

                if frame.is_end_headers() {
                    let (block, end_stream) = {
                        let stream = self.streams.get_mut(&stream_id).ok_or_else(|| {
                            ProtocolError::H2ProtocolError(
                                "Missing stream state for CONTINUATION frame".to_string(),
                            )
                        })?;
                        let pending = stream.pending_headers.take().ok_or_else(|| {
                            ProtocolError::H2ProtocolError(
                                "CONTINUATION frame without pending header block".to_string(),
                            )
                        })?;
                        let end_stream = pending.end_stream;
                        (pending.block.freeze(), end_stream)
                    };

                    // decode_header_block will update final_headers_received as needed.
                    let event = self.decode_header_block(stream_id, block.as_ref(), end_stream)?;
                    Ok(Some(event))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    fn header_fragment_bytes(&self, frame: &FrameH2) -> Result<Bytes, ProtocolError> {
        let payload = &frame.payload;
        let mut offset = 0usize;
        let mut pad_length = 0usize;

        if (frame.flags & PADDED_FLAG) != 0 {
            if payload.is_empty() {
                return Err(ProtocolError::H2ProtocolError(
                    "PADDED flag set but no pad length available".to_string(),
                ));
            }
            pad_length = payload[0] as usize;
            offset += 1;
            if pad_length > payload.len().saturating_sub(offset) {
                return Err(ProtocolError::H2ProtocolError(
                    "Invalid padding length in HEADERS frame".to_string(),
                ));
            }
        }

        if (frame.flags & PRIORITY_FLAG) != 0 {
            if payload.len() < offset + 5 {
                return Err(ProtocolError::H2ProtocolError(
                    "PRIORITY flag set but insufficient payload".to_string(),
                ));
            }
            offset += 5;
        }

        if pad_length > payload.len().saturating_sub(offset) {
            return Err(ProtocolError::H2ProtocolError(
                "Padding exceeds payload size".to_string(),
            ));
        }

        let end = payload.len() - pad_length;
        if offset > end {
            return Err(ProtocolError::H2ProtocolError(
                "Invalid header fragment boundaries".to_string(),
            ));
        }

        Ok(payload.slice(offset..end))
    }

    fn decode_header_block(
        &mut self,
        stream_id: u32,
        block: &[u8],
        end_stream: bool,
    ) -> Result<StreamEvent, ProtocolError> {
        let headers = self.hpack.decode(block)?;

        let status_code = headers.iter().find_map(|h| {
            (h.name == ":status")
                .then(|| h.value.as_ref()?.parse::<u16>().ok())
                .flatten()
        });

        let informational = status_code.map(|code| code < 200).unwrap_or(false);

        let already_final = self
            .streams
            .get(&stream_id)
            .map(|s| s.final_headers_received)
            .unwrap_or(false);

        if !informational && !already_final {
            if let Some(stream) = self.streams.get_mut(&stream_id) {
                stream.final_headers_received = true;
            }
        }

        let is_trailer = already_final && !informational;

        Ok(StreamEvent::Headers {
            headers,
            end_stream,
            is_trailer,
        })
    }

    fn data_payload(frame: &FrameH2) -> Result<Bytes, ProtocolError> {
        let payload = &frame.payload;
        if (frame.flags & PADDED_FLAG) == 0 {
            return Ok(payload.clone());
        }

        if payload.is_empty() {
            return Err(ProtocolError::H2ProtocolError(
                "DATA frame with PADDED flag set but empty payload".to_string(),
            ));
        }

        let pad_length = payload[0] as usize;
        if pad_length > payload.len().saturating_sub(1) {
            return Err(ProtocolError::H2ProtocolError(
                "Padding length exceeds DATA payload".to_string(),
            ));
        }

        let end = payload.len() - pad_length;
        Ok(payload.slice(1..end))
    }
    pub async fn send_frame(&mut self, frame: &FrameH2) -> Result<(), ProtocolError> {
        let serialized = frame.serialize()?;
        self.queue_serialized_frame(serialized).await
    }

    async fn queue_serialized_frame(&mut self, serialized: Bytes) -> Result<(), ProtocolError> {
        self.pending_write_bytes += serialized.len();
        self.pending_writes.push(serialized);

        let should_flush = match self.auto_flush_bytes {
            Some(threshold) => self.pending_write_bytes >= threshold,
            None => true,
        };

        if should_flush {
            self.flush_pending_writes().await?;
        }

        Ok(())
    }

    async fn read_frame_from_wire(&mut self) -> Result<FrameH2, ProtocolError> {
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

        FrameH2::parse(&frame_buf)
    }

    async fn write_to_stream(&mut self, data: &[u8]) -> Result<(), ProtocolError> {
        let write_timeout = self.timeouts.write;
        timeout_result(write_timeout, async {
            match &mut self.stream {
                TransportStream::Tcp(tcp) => tcp.write_all(data).await.map_err(ProtocolError::Io),
                TransportStream::Tls(tls) => tls.write_all(data).await.map_err(ProtocolError::Io),
            }
        })
        .await
    }

    async fn read_from_stream(&mut self, buffer: &mut [u8]) -> Result<usize, ProtocolError> {
        let read_timeout = self.timeouts.read;
        timeout_result(read_timeout, async {
            match &mut self.stream {
                TransportStream::Tcp(tcp) => {
                    tcp.read_exact(buffer).await.map_err(ProtocolError::Io)?;
                }
                TransportStream::Tls(tls) => {
                    tls.read_exact(buffer).await.map_err(ProtocolError::Io)?;
                }
            }
            Ok(buffer.len())
        })
        .await
    }

    pub fn is_connection_open(&self) -> bool {
        matches!(
            self.state,
            ConnectionState::Open | ConnectionState::HalfClosedRemote
        )
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

    pub async fn read_response(self: &mut Self, stream_id: u32) -> Result<Response, ProtocolError> {
        self.read_response_options(stream_id, None, None, None, None)
            .await
    }

    pub async fn read_response_options(
        self: &mut Self,
        stream_id: u32,
        overall_timeout: Option<Duration>,
        event_timeout: Option<Duration>,
        max_events: Option<usize>,
        event_handler: Option<&dyn Fn(&StreamEvent)>,
    ) -> Result<Response, ProtocolError> {
        let protocol = "HTTP/2.0".to_string();
        let mut status: Option<u16> = None;
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut trailers: Option<Vec<Header>> = None;
        let mut event_count = 0;
        let start_time = std::time::Instant::now();

        loop {
            // Check overall timeout
            if let Some(timeout) = overall_timeout {
                if start_time.elapsed() >= timeout {
                    break;
                }
            }

            // Check max events limit
            if let Some(max) = max_events {
                if event_count >= max {
                    break;
                }
            }

            // Read event with optional timeout
            let event_result = if let Some(timeout) = event_timeout {
                match tokio::time::timeout(timeout, self.recv_stream_event(stream_id)).await {
                    Ok(result) => result,
                    Err(_) => break, // Timeout - no more events
                }
            } else {
                self.recv_stream_event(stream_id).await
            };

            let event = match event_result {
                Ok(event) => {
                    event_count += 1;

                    // Call event handler if provided
                    if let Some(handler) = event_handler {
                        handler(&event);
                    }

                    event
                }
                Err(e) => {
                    if event_handler.is_some() {
                        // If we have a handler, this might be expected (like for testing)
                        break;
                    } else {
                        return Err(e);
                    }
                }
            };

            match event {
                StreamEvent::Headers {
                    headers: block,
                    end_stream,
                    is_trailer,
                } => {
                    if !is_trailer {
                        let mut parsed_status: Option<u16> = None;
                        let mut filtered = Vec::new();
                        for header in block.into_iter() {
                            if header.name == ":status" {
                                if let Some(ref value) = header.value {
                                    if let Ok(code) = value.parse::<u16>() {
                                        parsed_status = Some(code);
                                    }
                                }
                            } else if !header.name.starts_with(':') {
                                filtered.push(header);
                            }
                        }

                        let code = parsed_status.ok_or_else(|| {
                            ProtocolError::InvalidResponse(
                                "Missing :status header in response".to_string(),
                            )
                        })?;

                        if code < 200 {
                            if end_stream {
                                return Err(ProtocolError::InvalidResponse(
                                    "Informational response closed stream".to_string(),
                                ));
                            }
                            continue;
                        }

                        status = Some(code);
                        headers = filtered;

                        if end_stream {
                            break;
                        }
                    } else {
                        let trailer_headers = trailers.get_or_insert_with(Vec::new);
                        trailer_headers
                            .extend(block.into_iter().filter(|h| !h.name.starts_with(':')));
                        if end_stream {
                            break;
                        }
                    }
                }
                StreamEvent::Data {
                    payload,
                    end_stream,
                } => {
                    body.extend_from_slice(&payload);
                    if end_stream {
                        break;
                    }
                }
                StreamEvent::RstStream { error_code } => {
                    return Err(ProtocolError::H2StreamError(H2StreamErrorKind::Reset(
                        error_code,
                    )));
                }
            }
        }

        let status = status.ok_or_else(|| {
            ProtocolError::InvalidResponse("No final response received".to_string())
        })?;

        let cookies = Response::collect_cookies(&headers);

        Ok(Response {
            status,
            protocol,
            headers,
            body: Bytes::from(body),
            trailers,
            frames: self
                .take_captured_frames(stream_id)
                .map(|frames| frames.into_iter().map(ResponseFrame::Http2).collect()),
            cookies,
        })
    }
}

#[async_trait(?Send)]
impl FrameSink<FrameH2> for H2Connection {
    async fn write_frame(&mut self, frame: FrameH2) -> Result<(), ProtocolError> {
        let serialized = frame.serialize()?;
        self.queue_serialized_frame(serialized).await
    }
}

impl H2Connection {
    fn record_frame(&mut self, frame: &FrameH2) {
        if frame.stream_id == 0 {
            return;
        }

        self.captured_frames
            .entry(frame.stream_id)
            .or_default()
            .push(frame.clone());
    }

    fn take_captured_frames(&mut self, stream_id: u32) -> Option<Vec<FrameH2>> {
        self.captured_frames.remove(&stream_id)
    }
}

#[async_trait(?Send)]
impl HttpConnection for H2Connection {
    type ConnectOptions = H2ConnectOptions;
    type ReadOptions = u32;

    async fn connect(options: Self::ConnectOptions) -> Result<Self, ProtocolError> {
        H2Connection::connect(&options.target, &options.timeouts).await
    }

    async fn read_response(
        &mut self,
        stream_id: Self::ReadOptions,
    ) -> Result<Response, ProtocolError> {
        H2Connection::read_response(self, stream_id).await
    }
}
