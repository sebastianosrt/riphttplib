use bytes::BytesMut;
use quinn::RecvStream;

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Idle,
    Open,
    Closed,
}

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
    pub recv_stream: RecvStream,
    pub recv_buf: BytesMut,
}

impl StreamInfo {
    pub(super) fn new(recv_stream: RecvStream) -> Self {
        Self {
            state: StreamState::Open,
            recv_stream,
            recv_buf: BytesMut::new(),
        }
    }
}
