mod state;

pub use state::{ConnectionState, StreamInfo, StreamState};

use crate::h3::consts::*;
use crate::h3::framing::{
    SETTINGS_MAX_FIELD_SECTION_SIZE, SETTINGS_QPACK_BLOCKED_STREAMS,
    SETTINGS_QPACK_MAX_TABLE_CAPACITY,
};
use crate::h3::qpack::{QpackDecodeStatus, SharedQpackState};
use crate::stream::NoCertificateVerification;
use crate::types::{
    FrameH3, FrameSink, FrameType, FrameTypeH3, H3StreamErrorKind, Header, ProtocolError, Target,
};
use crate::utils::parse_target;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use quinn::{ClientConfig as QuinnClientConfig, Connection, Endpoint, RecvStream, SendStream};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;

use quinn::crypto::rustls::QuicClientConfig;
use rustls::crypto::ring::default_provider;
use rustls::ClientConfig;
use std::sync::Arc;
use tokio::net::lookup_host;
use tokio::time::{timeout, Duration};

enum QpackStreamRole {
    Encoder,
    Decoder,
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
    control_recv_buf: BytesMut,
    pub qpack: SharedQpackState,
    // QPACK unidirectional streams (optional in this minimal implementation)
    pub qpack_encoder_recv: Option<RecvStream>,
    pub qpack_decoder_recv: Option<RecvStream>,
}

impl H3Connection {
    pub async fn create_quic_connection(
        host: &str,
        port: u16,
        server_name: &str,
    ) -> io::Result<Connection> {
        let _ = default_provider().install_default();

        let mut rustls_config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
            .with_no_client_auth();
        rustls_config.alpn_protocols = vec![b"h3".to_vec()];

        let quic_crypto = QuicClientConfig::try_from(rustls_config)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let client_config = QuinnClientConfig::new(Arc::new(quic_crypto));

        // Resolve hostname to addresses (DNS)
        let resolved_addrs: Vec<SocketAddr> = lookup_host((host, port))
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("DNS lookup failed for {}:{}: {}", host, port, e),
                )
            })?
            .collect();

        if resolved_addrs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("No addresses found for {}:{}", host, port),
            ));
        }

        let mut addrs = resolved_addrs;
        addrs.sort_by_key(|addr| if addr.is_ipv4() { 0 } else { 1 });

        let mut last_error: Option<io::Error> = None;

        for addr in addrs {
            let bind_addr = if addr.is_ipv4() {
                SocketAddr::from(([0, 0, 0, 0], 0))
            } else {
                SocketAddr::from(([0u16; 8], 0))
            };

            let mut endpoint = Endpoint::client(bind_addr)?;
            endpoint.set_default_client_config(client_config.clone());

            match endpoint.connect(addr, server_name) {
                Ok(connecting) => match connecting.await {
                    Ok(connection) => return Ok(connection),
                    Err(e) => {
                        last_error = Some(io::Error::new(io::ErrorKind::ConnectionRefused, e));
                    }
                },
                Err(e) => {
                    last_error = Some(io::Error::new(io::ErrorKind::ConnectionRefused, e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::ConnectionRefused,
                format!("Unable to connect to {}:{}", host, port),
            )
        }))
    }

    pub async fn connect(target: &str) -> Result<Self, ProtocolError> {
        let target = parse_target(target)?;
        Self::connect_with_target(&target).await
    }

    pub(crate) async fn connect_with_target(target: &Target) -> Result<Self, ProtocolError> {
        Self::connect_inner(target).await
    }

    async fn connect_inner(target: &Target) -> Result<Self, ProtocolError> {
        let host = target
            .host()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing host".to_string()))?;
        let port = target
            .port()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing port".to_string()))?;

        let connection = H3Connection::create_quic_connection(host, port, host)
            .await
            .map_err(|e| ProtocolError::ConnectionFailed(e.to_string()))?;

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
        let qpack = SharedQpackState::new(
            settings[&SETTINGS_QPACK_MAX_TABLE_CAPACITY],
            settings[&SETTINGS_QPACK_BLOCKED_STREAMS],
        );

        Self {
            connection,
            state: ConnectionState::Idle,
            settings,
            remote_settings,
            streams: HashMap::new(),
            next_stream_id: 0, // Client uses stream IDs 0, 4, 8, 12, ... (bidirectional client-initiated)
            control_send_stream: None,
            control_recv_stream: None,
            control_recv_buf: BytesMut::new(),
            qpack,
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
        if let Ok(mut enc_send) = self.connection.open_uni().await {
            Self::send_stream_type_direct(&mut enc_send, 0x02).await?;
            self.qpack.set_encoder_send(enc_send).await;
        }
        if let Ok(mut dec_send) = self.connection.open_uni().await {
            Self::send_stream_type_direct(&mut dec_send, 0x03).await?;
            self.qpack.set_decoder_send(dec_send).await;
        }

        // 4. Send initial SETTINGS frame
        FrameH3::settings(&[
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
        ])
        .send(self)
        .await?;

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

        // Expect initial SETTINGS frame from peer control stream
        if let Some(frame) = self.read_control_frame_blocking().await? {
            if !matches!(frame.frame_type, FrameType::H3(FrameTypeH3::Settings)) {
                return Err(ProtocolError::InvalidResponse(
                    "First control frame must be SETTINGS".to_string(),
                ));
            }
            self.handle_frame(&frame).await?;
        } else {
            return Err(ProtocolError::InvalidResponse(
                "Control stream closed before SETTINGS".to_string(),
            ));
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

    async fn send_stream_type_direct(
        stream: &mut SendStream,
        stream_type: u64,
    ) -> Result<(), ProtocolError> {
        let mut buf = Vec::new();
        Self::encode_varint_to_vec(&mut buf, stream_type);
        stream.write_all(&buf).await.map_err(|e| {
            ProtocolError::ConnectionFailed(format!("Failed to send stream type: {}", e))
        })?;
        Ok(())
    }

    async fn read_control_frame_blocking(&mut self) -> Result<Option<FrameH3>, ProtocolError> {
        self.read_control_frame_internal(true).await
    }

    async fn try_read_control_frame(&mut self) -> Result<Option<FrameH3>, ProtocolError> {
        if let Some(frame) = self.read_control_frame_internal(false).await? {
            return Ok(Some(frame));
        }

        let recv_stream = match self.control_recv_stream.as_mut() {
            Some(stream) => stream,
            None => return Ok(None),
        };

        let mut local_chunk = [0u8; 8192];
        match timeout(Duration::from_millis(0), recv_stream.read(&mut local_chunk)).await {
            Ok(Ok(Some(0))) => Ok(None),
            Ok(Ok(Some(n))) => {
                self.control_recv_buf.extend_from_slice(&local_chunk[..n]);
                self.read_control_frame_internal(false).await
            }
            Ok(Ok(None)) => {
                self.control_recv_stream = None;
                Ok(None)
            }
            Ok(Err(e)) => Err(ProtocolError::ConnectionFailed(format!(
                "Failed to read from control stream: {}",
                e
            ))),
            Err(_) => Ok(None),
        }
    }

    async fn read_control_frame_internal(
        &mut self,
        wait: bool,
    ) -> Result<Option<FrameH3>, ProtocolError> {
        let recv_stream = match self.control_recv_stream.as_mut() {
            Some(stream) => stream,
            None => return Ok(None),
        };

        let mut local_chunk = vec![0u8; 8192];
        loop {
            if let Some((frame, consumed)) = Self::try_parse_frame(&self.control_recv_buf, 0)? {
                let _ = self.control_recv_buf.split_to(consumed);
                return Ok(Some(frame));
            }

            if !wait {
                return Ok(None);
            }

            let n_opt = recv_stream.read(&mut local_chunk).await.map_err(|e| {
                ProtocolError::ConnectionFailed(format!(
                    "Failed to read from control stream: {}",
                    e
                ))
            })?;

            match n_opt {
                Some(0) => continue,
                Some(n) => {
                    self.control_recv_buf.extend_from_slice(&local_chunk[..n]);
                }
                None => {
                    self.control_recv_stream = None;
                    return Ok(None);
                }
            }
        }
    }

    async fn poll_qpack_stream(
        qpack: &SharedQpackState,
        stream: &mut Option<RecvStream>,
        role: QpackStreamRole,
        wait: bool,
    ) -> Result<bool, ProtocolError> {
        let recv_stream = match stream.as_mut() {
            Some(stream) => stream,
            None => return Ok(false),
        };

        let chunk_result = if wait {
            recv_stream.read_chunk(usize::MAX, true).await
        } else {
            match timeout(
                Duration::from_millis(0),
                recv_stream.read_chunk(usize::MAX, true),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => return Ok(false),
            }
        };

        match chunk_result {
            Ok(Some(chunk)) => {
                let bytes = chunk.bytes;
                match role {
                    QpackStreamRole::Encoder => qpack.handle_encoder_stream_bytes(bytes).await?,
                    QpackStreamRole::Decoder => qpack.handle_decoder_stream_bytes(bytes).await?,
                }
                Ok(true)
            }
            Ok(None) => {
                *stream = None;
                Ok(false)
            }
            Err(e) => Err(ProtocolError::H3StreamError(
                H3StreamErrorKind::ProtocolViolation(format!("Failed to read QPACK stream: {}", e)),
            )),
        }
    }

    async fn pump_qpack_encoder(&mut self, wait: bool) -> Result<bool, ProtocolError> {
        Self::poll_qpack_stream(
            &self.qpack,
            &mut self.qpack_encoder_recv,
            QpackStreamRole::Encoder,
            wait,
        )
        .await
    }

    async fn pump_qpack_decoder(&mut self) -> Result<bool, ProtocolError> {
        Self::poll_qpack_stream(
            &self.qpack,
            &mut self.qpack_decoder_recv,
            QpackStreamRole::Decoder,
            false,
        )
        .await
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

    async fn send_control_frame(&mut self, frame: &FrameH3) -> Result<(), ProtocolError> {
        if let Some(ref mut send_stream) = self.control_send_stream {
            let serialized = frame.serialize()?;

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
        self.next_stream_id += CLIENT_BIDI_STREAM_INCREMENT;

        let (send_stream, recv_stream) = self.connection.open_bi().await.map_err(|e| {
            ProtocolError::ConnectionFailed(format!("Failed to open request stream: {}", e))
        })?;

        self.streams.insert(stream_id, StreamInfo::new(recv_stream));

        Ok((stream_id, send_stream))
    }

    pub async fn read_request_frame(
        &mut self,
        stream_id: u32,
    ) -> Result<Option<FrameH3>, ProtocolError> {
        let stream_info = self.streams.get_mut(&stream_id).ok_or_else(|| {
            ProtocolError::RequestFailed(format!("Unknown request stream {}", stream_id))
        })?;

        // Maintain a per-stream buffer across reads
        let recv_stream = &mut stream_info.recv_stream;
        let mut local_chunk = vec![0u8; 8192];
        let buf = &mut stream_info.recv_buf;

        loop {
            // Try to parse a frame from current buffer
            if let Some((frame, consumed)) = Self::try_parse_frame(&*buf, stream_id)? {
                // advance buffer by consumed (drain)
                let _ = buf.split_to(consumed);
                return Ok(Some(frame));
            }

            // Need more data
            let n_opt = recv_stream.read(&mut local_chunk).await.map_err(|e| {
                ProtocolError::ConnectionFailed(format!(
                    "Failed to read from request stream {}: {}",
                    stream_id, e
                ))
            })?;

            match n_opt {
                Some(0) => continue,
                Some(n) => {
                    buf.extend_from_slice(&local_chunk[..n]);
                }
                None => {
                    if buf.is_empty() {
                        return Ok(None);
                    } else {
                        return Err(ProtocolError::InvalidResponse(
                            "Stream closed with incomplete frame".to_string(),
                        ));
                    }
                }
            }
        }
    }

    pub async fn poll_control(&mut self) -> Result<(), ProtocolError> {
        while let Some(frame) = self.try_read_control_frame().await? {
            self.handle_frame(&frame).await?;
        }
        Ok(())
    }

    pub async fn encode_headers(
        &self,
        stream_id: u32,
        headers: &[Header],
    ) -> Result<Bytes, ProtocolError> {
        self.qpack.encode_headers(stream_id as u64, headers).await
    }

    pub async fn decode_headers(
        &mut self,
        stream_id: u32,
        payload: &[u8],
    ) -> Result<Vec<Header>, ProtocolError> {
        // Process any pending QPACK feedback
        let _ = self.pump_qpack_decoder().await?;
        let _ = self.pump_qpack_encoder(false).await?;

        match self.qpack.decode_headers(stream_id as u64, payload).await? {
            QpackDecodeStatus::Complete(headers) => return Ok(headers),
            QpackDecodeStatus::Blocked => loop {
                let progressed = self.pump_qpack_encoder(true).await?;
                if !progressed {
                    return Err(ProtocolError::H3QpackError(
                        "Decoder remained blocked".to_string(),
                    ));
                }

                if let Some(status) = self.qpack.poll_unblocked(stream_id as u64).await? {
                    match status {
                        QpackDecodeStatus::Complete(headers) => return Ok(headers),
                        QpackDecodeStatus::Blocked => continue,
                    }
                }
            },
        }
    }

    fn try_parse_frame(
        buf: &BytesMut,
        stream_id: u32,
    ) -> Result<Option<(FrameH3, usize)>, ProtocolError> {
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
            other => FrameTypeH3::Unknown(other),
        };

        let frame = FrameH3 {
            frame_type: FrameType::H3(frame_type),
            stream_id,
            payload,
        };
        Ok(Some((frame, header_len + len as usize)))
    }

    pub async fn handle_frame(&mut self, frame: &FrameH3) -> Result<(), ProtocolError> {
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

    async fn handle_settings_frame(&mut self, frame: &FrameH3) -> Result<(), ProtocolError> {
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

        let max_table = *self
            .remote_settings
            .get(&SETTINGS_QPACK_MAX_TABLE_CAPACITY)
            .unwrap_or(&0);
        let max_blocked = *self
            .remote_settings
            .get(&SETTINGS_QPACK_BLOCKED_STREAMS)
            .unwrap_or(&0);
        self.qpack.configure_encoder(max_table, max_blocked).await?;
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

    async fn handle_headers_frame(&mut self, frame: &FrameH3) -> Result<(), ProtocolError> {
        // Update stream state to indicate headers have been received
        if let Some(stream_info) = self.streams.get_mut(&frame.stream_id) {
            match stream_info.state {
                StreamState::Open => {
                    // First headers frame (response headers)
                    stream_info.state = StreamState::HalfClosedLocal;
                }
                StreamState::HalfClosedLocal => {
                    // Could be trailers or continuation headers
                }
                _ => {
                    return Err(ProtocolError::H3StreamError(
                        H3StreamErrorKind::ProtocolViolation(format!(
                            "Received headers on stream {} in invalid state {:?}",
                            frame.stream_id, stream_info.state
                        )),
                    ));
                }
            }
        }
        Ok(())
    }

    async fn handle_data_frame(&mut self, frame: &FrameH3) -> Result<(), ProtocolError> {
        // Validate stream state for data frames
        if let Some(stream_info) = self.streams.get_mut(&frame.stream_id) {
            match stream_info.state {
                StreamState::Open | StreamState::HalfClosedLocal => {
                    // Valid states to receive data
                }
                StreamState::HalfClosedRemote | StreamState::Closed => {
                    return Err(ProtocolError::H3StreamError(
                        H3StreamErrorKind::ProtocolViolation(format!(
                            "Received data on stream {} in invalid state {:?}",
                            frame.stream_id, stream_info.state
                        )),
                    ));
                }
                StreamState::Idle => {
                    return Err(ProtocolError::H3StreamError(
                        H3StreamErrorKind::ProtocolViolation(format!(
                            "Received data on idle stream {}",
                            frame.stream_id
                        )),
                    ));
                }
            }
        } else {
            return Err(ProtocolError::H3StreamError(
                H3StreamErrorKind::ProtocolViolation(format!(
                    "Received data on unknown stream {}",
                    frame.stream_id
                )),
            ));
        }

        // Data frames don't change stream state by themselves
        // Stream state changes occur when the stream is closed by the sender
        Ok(())
    }

    async fn handle_goaway_frame(&mut self, _frame: &FrameH3) -> Result<(), ProtocolError> {
        self.state = ConnectionState::Closed;
        Ok(())
    }

    pub async fn send_goaway(&mut self, stream_id: u64) -> Result<(), ProtocolError> {
        FrameH3::goaway(stream_id).send(self).await?;
        self.state = ConnectionState::Closed;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        matches!(self.state, ConnectionState::Open)
    }

    pub async fn close(&mut self) -> Result<(), ProtocolError> {
        self.send_goaway(self.next_stream_id as u64).await
    }

    pub fn close_stream(&mut self, stream_id: u32) -> Result<(), ProtocolError> {
        if let Some(stream_info) = self.streams.get_mut(&stream_id) {
            match stream_info.state {
                StreamState::Open => {
                    stream_info.state = StreamState::HalfClosedLocal;
                }
                StreamState::HalfClosedRemote => {
                    stream_info.state = StreamState::Closed;
                }
                StreamState::HalfClosedLocal | StreamState::Closed => {
                    // Already closed or closing
                }
                StreamState::Idle => {
                    return Err(ProtocolError::H3StreamError(
                        H3StreamErrorKind::ProtocolViolation(format!(
                            "Cannot close idle stream {}",
                            stream_id
                        )),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn stream_finished_receiving(&mut self, stream_id: u32) -> Result<(), ProtocolError> {
        if let Some(stream_info) = self.streams.get_mut(&stream_id) {
            match stream_info.state {
                StreamState::Open => {
                    stream_info.state = StreamState::HalfClosedRemote;
                }
                StreamState::HalfClosedLocal => {
                    stream_info.state = StreamState::Closed;
                }
                StreamState::HalfClosedRemote | StreamState::Closed => {
                    // Already closed from remote side
                }
                StreamState::Idle => {
                    return Err(ProtocolError::H3StreamError(
                        H3StreamErrorKind::ProtocolViolation(format!(
                            "Stream {} was never opened",
                            stream_id
                        )),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn get_stream_state(&self, stream_id: u32) -> Option<StreamState> {
        self.streams.get(&stream_id).map(|info| info.state.clone())
    }

    pub fn remove_closed_stream(&mut self, stream_id: u32) -> bool {
        if let Some(stream_info) = self.streams.get(&stream_id) {
            if matches!(stream_info.state, StreamState::Closed) {
                self.streams.remove(&stream_id);
                return true;
            }
        }
        false
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

#[async_trait(?Send)]
impl FrameSink<FrameH3> for H3Connection {
    async fn write_frame(&mut self, frame: FrameH3) -> Result<(), ProtocolError> {
        match &frame.frame_type {
            FrameType::H3(inner) => match inner {
                FrameTypeH3::Settings
                | FrameTypeH3::CancelPush
                | FrameTypeH3::GoAway
                | FrameTypeH3::MaxPushId => {
                    if frame.stream_id != 0 {
                        return Err(ProtocolError::H3MessageError(
                            "Control frames must target stream 0".to_string(),
                        ));
                    }
                    self.send_control_frame(&frame).await
                }
                _ => Err(ProtocolError::H3MessageError(
                    "Use stream-bound APIs to send request frames".to_string(),
                )),
            },
            FrameType::H2(_) => Err(ProtocolError::H3MessageError(
                "Attempted to send HTTP/2 frame via an HTTP/3 connection".to_string(),
            )),
        }
    }
}
