use crate::types::{H2ErrorCode, Header};
use bytes::{Bytes, BytesMut};
use std::collections::VecDeque;

// Connection States
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Idle,
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
}

// Stream States (RFC 7540 Section 5.1)
#[derive(Debug, Clone, PartialEq)]
pub enum StreamState {
    Idle,
    Open,
    HalfClosedLocal,
    HalfClosedRemote,
    Closed,
}

impl std::fmt::Display for StreamState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamState::Idle => write!(f, "idle"),
            StreamState::Open => write!(f, "open"),
            StreamState::HalfClosedLocal => write!(f, "half-closed (local)"),
            StreamState::HalfClosedRemote => write!(f, "half-closed (remote)"),
            StreamState::Closed => write!(f, "closed"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Headers {
        headers: Vec<Header>,
        end_stream: bool,
        is_trailer: bool,
    },
    Data {
        payload: Bytes,
        end_stream: bool,
    },
    RstStream {
        error_code: H2ErrorCode,
    },
}

#[derive(Debug, Clone)]
pub(super) struct PendingHeaderBlock {
    pub(super) block: BytesMut,
    pub(super) end_stream: bool,
}

impl PendingHeaderBlock {
    pub(super) fn new() -> Self {
        Self {
            block: BytesMut::new(),
            end_stream: false,
        }
    }

    pub(super) fn append(&mut self, fragment: &[u8]) {
        self.block.extend_from_slice(fragment);
    }
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub state: StreamState,
    pub send_window: i32,
    pub recv_window: i32,
    pub headers_sent: bool,
    pub final_headers_received: bool,
    pub end_stream_received: bool,
    pub end_stream_sent: bool,
    pub inbound_events: VecDeque<StreamEvent>,
    pub(super) pending_headers: Option<PendingHeaderBlock>,
}

impl StreamInfo {
    pub(super) fn new(send_window: i32, recv_window: i32) -> Self {
        Self {
            state: StreamState::Idle,
            send_window,
            recv_window,
            headers_sent: false,
            final_headers_received: false,
            end_stream_received: false,
            end_stream_sent: false,
            inbound_events: VecDeque::new(),
            pending_headers: None,
        }
    }
}
