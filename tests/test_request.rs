use bytes::Bytes;
use riphttplib::types::{ClientTimeouts, Header, ProxySettings, Request};
use serde_json::json;
use std::time::Duration;
use url::Url;

#[test]
fn request_builder_sets_fields() {
    let request = Request::new("http://example.com/", "POST")
        .expect("valid request")
        .with_header(Header::new(
            "content-type".to_string(),
            "application/json".to_string(),
        ))
        .with_optional_body(Some(Bytes::from("hello")))
        .with_trailers(Some(vec![Header::new(
            "x-checksum".to_string(),
            "abc".to_string(),
        )]));

    assert_eq!(request.method, "POST");
    assert_eq!(request.headers.len(), 1);
    assert_eq!(request.body.as_ref().unwrap(), &Bytes::from("hello"));
    assert!(request.trailers.as_ref().unwrap()[0].name.starts_with("x-"));
    assert!(request.params.is_empty());
    assert!(request.cookies.is_empty());
    assert!(request.allow_redirects);
    assert!(!request.stream);
}

#[test]
fn request_with_optional_body_none() {
    let request = Request::new("http://example.com/", "GET")
        .expect("valid request")
        .with_optional_body::<Bytes>(None);
    assert!(request.body.is_none());
}

#[test]
fn request_with_params_appends_to_path() {
    let request = Request::new("http://example.com/api", "GET")
        .expect("valid request")
        .with_params(vec![("search", "rust crates"), ("page", "1")]);

    assert_eq!(request.path(), "/api?search=rust+crates&page=1");
    assert_eq!(request.params.len(), 2);
}

#[test]
fn request_with_params_preserves_existing_query() {
    let request = Request::new("http://example.com/api?foo=1", "GET")
        .expect("valid request")
        .with_params(vec![("bar", "2")]);

    assert_eq!(request.path(), "/api?foo=1&bar=2");
}

#[test]
fn request_with_json_sets_body_and_header() {
    let request = Request::new("http://example.com/", "POST")
        .expect("valid request")
        .with_json(json!({ "ok": true }));

    let body = request.body.as_ref().expect("json body is present");
    assert_eq!(body, &Bytes::from(r#"{"ok":true}"#));

    let headers = request.effective_headers();
    let content_type = headers
        .iter()
        .find(|h| h.name == "content-type")
        .and_then(|h| h.value.as_ref());

    assert_eq!(content_type, Some(&"application/json".to_string()));
    assert!(request.json.is_some());
}

#[test]
fn request_with_cookies_adds_cookie_header() {
    let request = Request::new("http://example.com/", "GET")
        .expect("valid request")
        .with_cookies(vec![("session", "abc123"), ("theme", "dark")]);

    let headers = request.effective_headers();
    let cookie = headers
        .iter()
        .find(|h| h.name == "cookie")
        .and_then(|h| h.value.as_ref())
        .expect("cookie header exists");

    assert_eq!(cookie, "session=abc123; theme=dark");
}

#[test]
fn request_with_timeout_and_flags() {
    let custom_timeouts = ClientTimeouts {
        connect: Some(Duration::from_secs(1)),
        read: Some(Duration::from_secs(2)),
        write: Some(Duration::from_secs(3)),
    };

    let request = Request::new("http://example.com/", "GET")
        .expect("valid request")
        .with_timeout(custom_timeouts.clone())
        .with_allow_redirects(false)
        .with_stream(true);

    assert_eq!(request.timeout.as_ref(), Some(&custom_timeouts));
    assert!(!request.allow_redirects);
    assert!(request.stream);

    let merged = request.effective_timeouts(&ClientTimeouts::default());
    assert_eq!(merged.connect, custom_timeouts.connect);
}

#[test]
fn request_with_proxies_sets_field() {
    let http_proxy = Url::parse("http://localhost:8080").expect("valid proxy url");
    let proxies = ProxySettings {
        http: Some(http_proxy.clone()),
        https: None,
    };

    let request = Request::new("http://example.com/", "GET")
        .expect("valid request")
        .with_proxies(proxies.clone());

    let request_proxies = request.proxies.expect("proxies are set");
    assert_eq!(request_proxies.http, Some(http_proxy));
    assert!(request_proxies.https.is_none());
}
