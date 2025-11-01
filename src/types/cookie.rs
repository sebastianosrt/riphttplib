use super::Header;

// TODO maybe duplicate code
pub fn parse_set_cookie(input: &str) -> Option<(String, String)> {
    let mut parts = input.split(';');
    let pair = parts.next()?.trim();
    if pair.is_empty() {
        return None;
    }

    let mut kv = pair.splitn(2, '=');
    let name = kv.next()?.trim();
    if name.is_empty() {
        return None;
    }
    let value = kv.next().unwrap_or("").trim();
    Some((name.to_string(), value.to_string()))
}

pub fn extract_cookies(headers: &[Header]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|header| header.value.as_ref())
        .filter_map(|value| parse_set_cookie(value))
        .collect()
}
