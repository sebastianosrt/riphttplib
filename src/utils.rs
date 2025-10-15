use crate::{
    types::{Header, ProtocolError, Request, Target},
};
use bytes::Bytes;
use url::Url;

pub const USER_AGENT: &str = "riphttplib/0.1.0";
pub const CRLF: &str = "\r\n";
pub const HTTP_VERSION_1_1: &str = "HTTP/1.1";
pub const HTTP_VERSION_2_0: &str = "HTTP/2.0";
pub const HTTP_VERSION_3_0: &str = "HTTP/3.0";
pub const HOST_HEADER: &str = "host";
pub const CONTENT_LENGTH_HEADER: &str = "content-length";
pub const TRANSFER_ENCODING_HEADER: &str = "transfer-encoding";
pub const USER_AGENT_HEADER: &str = "user-agent";
pub const CHUNKED_ENCODING: &str = "chunked";

pub fn parse_target(target: &str) -> Result<Target, ProtocolError> {
    let url = Url::parse(target)
        .map_err(|e| ProtocolError::InvalidTarget(format!("{} ({})", target, e)))?;

    if url.host_str().is_none() {
        return Err(ProtocolError::InvalidTarget(format!(
            "Target '{}' is missing a host",
            target
        )));
    }

    if url.port_or_known_default().is_none() {
        return Err(ProtocolError::InvalidTarget(format!(
            "Target '{}' has no known port",
            target
        )));
    }

    Ok(Target::new(url))
}

pub fn convert_escape_sequences(input: &str) -> String {
    input
        .replace("\\\\", "\\")
        .replace("\\r", "\r")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
}

pub fn parse_header(header: &str) -> Option<Header> {
    if header.starts_with(':') {
        // For pseudo-headers, find the second colon
        if let Some(colon_pos) = header[1..].find(':') {
            let split_pos = colon_pos + 1;
            let name = header[..=split_pos - 1].to_string();
            let value = convert_escape_sequences(header[split_pos + 1..].trim_start());
            Some(Header::new(name.to_lowercase(), value))
        } else {
            Some(Header::new_valueless(header.to_string().to_lowercase()))
        }
    } else {
        // Regular header, split on first colon
        if let Some((name, value)) = header.split_once(':') {
            Some(Header::new(
                name.to_string().to_lowercase(),
                convert_escape_sequences(value.trim_start()),
            ))
        } else {
            // valueless header
            Some(Header::new_valueless(header.to_string().to_lowercase()))
        }
    }
}

pub fn normalize_headers(headers: &[Header]) -> Vec<Header> {
    headers
        .iter()
        .map(|h| Header {
            name: h.name.to_lowercase(),
            value: h.value.clone(),
        })
        .collect()
}

pub fn prepare_pseudo_headers(
    request: &Request,
    target: &Target,
) -> Result<Vec<Header>, ProtocolError> {
    let mut pseudo_headers: Vec<Header> = request
        .headers
        .iter()
        .filter(|h| h.name.starts_with(':'))
        .cloned()
        .collect();

    if !pseudo_headers.iter().any(|h| h.name == ":method") {
        pseudo_headers.insert(
            0,
            Header::new(":method".to_string(), request.method.clone()),
        );
    }

    let method = request.method.to_uppercase();
    match method.as_str() {
        "CONNECT" => {
            if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                let authority = target.authority().ok_or_else(|| {
                    ProtocolError::InvalidTarget(
                        "CONNECT requests require an authority".to_string(),
                    )
                })?;
                pseudo_headers.push(Header::new(":authority".to_string(), authority));
            }
            pseudo_headers.retain(|h| h.name != ":scheme" && h.name != ":path");
        }
        "OPTIONS" => {
            let path_value = if target.path_only() == "*" {
                "*".to_string()
            } else {
                target.path()
            };
            if !pseudo_headers.iter().any(|h| h.name == ":path") {
                pseudo_headers.push(Header::new(":path".to_string(), path_value));
            }
            if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                if let Some(authority) = target.authority() {
                    pseudo_headers.push(Header::new(":authority".to_string(), authority));
                }
            }
            if !pseudo_headers.iter().any(|h| h.name == ":scheme") {
                pseudo_headers.push(Header::new(
                    ":scheme".to_string(),
                    target.scheme().to_string(),
                ));
            }
        }
        _ => {
            if !pseudo_headers.iter().any(|h| h.name == ":path") {
                pseudo_headers.push(Header::new(":path".to_string(), target.path()));
            }
            if !pseudo_headers.iter().any(|h| h.name == ":scheme") {
                pseudo_headers.push(Header::new(
                    ":scheme".to_string(),
                    target.scheme().to_string(),
                ));
            }
            if !pseudo_headers.iter().any(|h| h.name == ":authority") {
                if let Some(authority) = target.authority() {
                    pseudo_headers.push(Header::new(":authority".to_string(), authority));
                }
            }
        }
    }

    Ok(normalize_headers(&pseudo_headers))
}

pub fn merge_headers(pseudo: Vec<Header>, request: &Request) -> Vec<Header> {
    let mut headers = Vec::with_capacity(pseudo.len() + request.headers.len());
    headers.extend(pseudo);
    headers.extend(
        request
            .headers
            .iter()
            .filter(|h| !h.name.starts_with(':'))
            .cloned()
            .map(|mut h| {
                h.name = h.name.to_lowercase();
                h
            }),
    );
    headers
}

pub fn build_request(
    method: impl Into<String>,
    headers: Vec<Header>,
    body: Option<Bytes>,
    trailers: Option<Vec<Header>>,
) -> Request {
    Request::new(method)
        .with_headers(headers)
        .with_optional_body(body)
        .with_trailers(trailers)
}
