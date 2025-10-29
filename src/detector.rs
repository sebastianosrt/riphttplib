use crate::h1::client::H1Client;
use crate::stream::create_stream;
use crate::types::protocol::HttpProtocol;
use crate::types::{ClientTimeouts, ProtocolError, Request, Target};
use crate::h2::connection::H2Connection;
use crate::utils::{header_value, parse_target};
use std::time::Duration;
use tokio::time::timeout;

const DETECTION_TIMEOUT: Duration = Duration::from_secs(3);

pub struct DetectedProtocol {
    pub protocol: HttpProtocol,
    pub port: Option<u16>,
}

pub fn extract_alt_svc_port(header: &str) -> Option<u16> {
    header
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
        .next()
}

async fn alt_svc_port(url: &str, timeouts: &ClientTimeouts) -> Option<u16> {
    let request = match Request::new(url, "HEAD") {
        Ok(req) => req.timeout(timeouts.clone()).allow_redirects(false),
        Err(_) => return None,
    };

    let client = H1Client::timeouts(timeouts.clone());
    match timeout(DETECTION_TIMEOUT, client.send_request(request)).await {
        Ok(Ok(response)) => header_value(&response.headers, "alt-svc")
            .and_then(extract_alt_svc_port),
        _ => None,
    }
}

pub async fn detect_protocol(url: &str, skip_h1: bool) -> Result<Vec<DetectedProtocol>, ProtocolError> {
    let target = parse_target(url)?;
    let scheme = target.scheme().to_string();
    let host = target.host().unwrap();
    let port = target.port().unwrap();
    let mut supported = Vec::new();

    let timeouts = ClientTimeouts {
        connect: Some(DETECTION_TIMEOUT),
        read: Some(DETECTION_TIMEOUT),
        write: Some(DETECTION_TIMEOUT),
    };

    // detect h1
    if skip_h1 || create_stream(&scheme, host, port, timeouts.connect).await.is_ok() {
        supported.push(DetectedProtocol {
            protocol: HttpProtocol::Http1,
            port: Some(port),
        });
    }
    // detect h2
    if H2Connection::connect(url, &timeouts).await.is_ok() {
        supported.push(DetectedProtocol {
            protocol: HttpProtocol::Http2,
            port: Some(port),
        });
    }
    // detect h3
    if let Some(port) = alt_svc_port(url, &timeouts).await {
        supported.push(DetectedProtocol {
            protocol: HttpProtocol::Http3,
            port: Some(port),
        });
    }

    Ok(supported)
}