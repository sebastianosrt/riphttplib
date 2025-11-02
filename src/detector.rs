use crate::h1::protocol::H1;
use crate::h2::connection::H2Connection;
use crate::types::protocol::HttpProtocol;
use crate::types::{ClientTimeouts, ProtocolError};
use crate::utils::{header_value, parse_target};
use crate::Client;
use std::time::Duration;

const DETECTION_TIMEOUT: Duration = Duration::from_secs(3);

pub struct DetectedProtocol {
    pub protocol: HttpProtocol,
    pub port: Option<u16>,
}

pub fn extract_alt_svc_port(header: Option<&str>) -> Option<u16> {
    match header {
        Some(header) => header
            .split(',')
            .filter_map(|entry| {
                let entry = entry.trim();
                if !entry.starts_with("h3") {
                    return None;
                }

                let start = entry.find('"')?;
                let end = entry[start + 1..].find('"')? + start + 1;
                let value = &entry[start + 1..end];

                let port_part = value.split(':').last()?;
                port_part.parse::<u16>().ok()
            })
            .next(),
        None => None,
    }
}

pub async fn detect_protocol(url: &str) -> Result<Vec<DetectedProtocol>, ProtocolError> {
    let target = parse_target(url)?;
    let scheme = target.scheme().to_string();
    let port = target.port().unwrap();
    let mut supported = Vec::new();

    let timeouts = ClientTimeouts {
        connect: Some(DETECTION_TIMEOUT),
        read: Some(DETECTION_TIMEOUT),
        write: Some(DETECTION_TIMEOUT),
    };

    // detect h1
    match Client::<H1>::head(&url).await {
        Ok(res) => {
            supported.push(DetectedProtocol {
                protocol: HttpProtocol::Http1,
                port: Some(port),
            });
            // detect from svc header h3
            if let Some(port) = extract_alt_svc_port(header_value(&res.headers, "alt-svc")) {
                supported.push(DetectedProtocol {
                    protocol: HttpProtocol::Http3,
                    port: Some(port),
                });
            }
        }
        Err(_) => {}
    }
    // detect h2
    if H2Connection::connect(url, &timeouts).await.is_ok() {
        supported.push(DetectedProtocol {
            protocol: if scheme == "http" {
                HttpProtocol::H2C
            } else {
                HttpProtocol::Http2
            },
            port: Some(port),
        });
    }

    Ok(supported)
}
