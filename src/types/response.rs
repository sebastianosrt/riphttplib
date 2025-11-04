use super::{extract_cookies, FrameH2, FrameH3, Header};
use bytes::Bytes;
use serde_json::Value;
use std::fmt::{self, Display, Formatter};

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
    pub cookies: Vec<(String, String)>,
}

impl Response {
    pub fn text(self: &Self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    pub fn json(self: &Self) -> Result<Value, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    pub fn collect_cookies(headers: &[Header]) -> Vec<(String, String)> {
        extract_cookies(headers)
    }
}

impl Display for Response {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "\n{} {}", self.protocol, self.status)?;
        for header in &self.headers {
            writeln!(f, "{}", header)?;
        }
        writeln!(f)?;
        write!(f, "{}", self.text())
    }
}
