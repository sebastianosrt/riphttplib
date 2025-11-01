use crate::types::{Header, ProtocolError, Request, Response, Target};
use std::future::Future;
use std::time::Duration;
use tokio::time::timeout;
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

// Common header names as constants to avoid allocations
pub const CONTENT_TYPE_HEADER: &str = "content-type";
pub const COOKIE_HEADER: &str = "cookie";
pub const APPLICATION_JSON: &str = "application/json";

pub fn ensure_user_agent(headers: &mut Vec<Header>) {
    if !headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case(USER_AGENT_HEADER))
    {
        headers.push(Header::new(
            USER_AGENT_HEADER.to_string(),
            USER_AGENT.to_string(),
        ));
    }
}

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
            Some(Header::new(name, value))
        } else {
            Some(Header::new_valueless(header.to_string()))
        }
    } else {
        // Regular header, split on first colon
        if let Some((name, value)) = header.split_once(':') {
            Some(Header::new(
                name.to_string(),
                convert_escape_sequences(value.trim_start()),
            ))
        // valueless header
        } else {
            Some(Header::new_valueless(header.to_string()))
        }
    }
}

// TODO write better
pub fn parse_headers(headers: Vec<String>) -> Vec<Header> {
    let mut out = Vec::new();
    for header in &headers {
        out.push(parse_header(header).unwrap());
    }
    out
}

pub fn header_value<'a>(headers: &'a [Header], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .and_then(|h| h.value.as_deref())
}

pub fn is_redirect_status(status: u16) -> bool {
    (300..400).contains(&status)
}

pub fn resolve_redirect_url(base_url: &Url, location: &str) -> Result<Url, url::ParseError> {
    if location.starts_with("http://") || location.starts_with("https://") {
        Url::parse(location)
    } else {
        base_url.join(location)
    }
}

pub fn apply_redirect(request: &mut Request, response: &Response) -> Result<bool, ProtocolError> {
    if !request.follow_redirects || !is_redirect_status(response.status) {
        return Ok(false);
    }

    let location = match header_value(&response.headers, "location") {
        Some(value) => value,
        None => return Ok(false),
    };

    let redirect_url = match resolve_redirect_url(&request.target.url, location) {
        Ok(url) => url,
        Err(_) => return Ok(false),
    };

    request.target = parse_target(redirect_url.as_str())?;

    if response.status == 303
        || ((response.status == 301 || response.status == 302)
            && matches!(request.method.as_str(), "GET" | "HEAD"))
    {
        request.method = "GET".to_string();
        request.body = None;
        request.json = None;
    }

    Ok(true)
}

pub async fn timeout_result<F, T>(duration: Option<Duration>, future: F) -> Result<T, ProtocolError>
where
    F: Future<Output = Result<T, ProtocolError>>,
{
    if let Some(dur) = duration {
        match timeout(dur, future).await {
            Ok(result) => result,
            Err(_) => Err(ProtocolError::Timeout),
        }
    } else {
        future.await
    }
}
