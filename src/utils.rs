use crate::types::{Header, Target};
use std::collections::HashSet;
use url::Url;

pub fn parse_target(target: &str) -> Target {
    let url = Url::parse(target).unwrap();
    Target {
        host: url.host_str().unwrap().to_string(),
        port: url.port_or_known_default().unwrap(),
        url: target.to_string(),
        protocols: HashSet::new(),
        scheme: url.scheme().to_string(),
        path: url.path().to_string(),
    }
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
            let split_pos = colon_pos + 1; // Position relative to the original string
            let name = header[..=split_pos - 1].to_string(); // Include up to the second colon
            let value = convert_escape_sequences(header[split_pos + 1..].trim_start()); // Skip the colon and trim
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
