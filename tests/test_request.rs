use bytes::Bytes;
use riphttplib::types::{Header, Request};

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
}

#[test]
fn request_with_optional_body_none() {
    let request = Request::new("http://example.com/", "GET")
        .expect("valid request")
        .with_optional_body::<Bytes>(None);
    assert!(request.body.is_none());
}
