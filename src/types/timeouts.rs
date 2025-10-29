use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct ClientTimeouts {
    pub connect: Option<Duration>,
    pub read: Option<Duration>,
    pub write: Option<Duration>,
}

impl Default for ClientTimeouts {
    fn default() -> Self {
        Self {
            connect: Some(Duration::from_secs(10)),
            read: Some(Duration::from_secs(30)),
            write: Some(Duration::from_secs(30)),
        }
    }
}

impl ClientTimeouts {
    pub fn disabled() -> Self {
        Self {
            connect: None,
            read: None,
            write: None,
        }
    }
}
