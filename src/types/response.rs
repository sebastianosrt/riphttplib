use super::{FrameH2, FrameH3, Header};
use bytes::Bytes;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum ResponseFrame {
    Http2(FrameH2),
    Http3(FrameH3),
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub protocol: String,
    pub headers: Vec<Header>,
    pub body: Bytes,
    pub trailers: Option<Vec<Header>>,
    pub frames: Option<Vec<ResponseFrame>>,
}

impl Response {
    pub fn text(self: &Self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    pub fn json(self: &Self) -> Result<Value, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }
}
