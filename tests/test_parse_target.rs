use riphttplib::types::ProtocolError;
use riphttplib::utils::parse_target;

#[test]
fn parse_target_with_default_ports() {
    let http = parse_target("http://example.com/path?q=1").expect("valid http");
    assert_eq!(http.scheme(), "http");
    assert_eq!(http.port().unwrap(), 80);
    assert_eq!(http.path(), "/path?q=1");

    let https = parse_target("https://example.com").expect("valid https");
    assert_eq!(https.port().unwrap(), 443);
    assert_eq!(https.path(), "/");
}

#[test]
fn parse_target_explicit_port() {
    let target = parse_target("http://localhost:8080/foo").expect("valid");
    assert_eq!(target.port().unwrap(), 8080);
    assert_eq!(target.authority().unwrap(), "localhost:8080");
}

#[test]
fn parse_target_invalid_inputs() {
    let err = parse_target("not a url").unwrap_err();
    assert!(matches!(err, ProtocolError::InvalidTarget(_)));

    let missing_host = parse_target("http://:8080").unwrap_err();
    assert!(matches!(missing_host, ProtocolError::InvalidTarget(_)));
}
