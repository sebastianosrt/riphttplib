#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn header_value<'a>(headers: &'a [Header], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| h.value.as_deref())
    }

    fn header_count(headers: &[Header], name: &str) -> usize {
        headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(name))
            .count()
    }

    #[test]
    fn path_merges_params_with_existing_query() -> Result<(), ProtocolError> {
        let request = Request::new("https://example.com/base?existing=1", "GET")?
            .params(vec![("foo", "bar baz"), ("multi", "value")]);

        assert_eq!(request.path(), "/base?existing=1&foo=bar+baz&multi=value");
        Ok(())
    }

    #[test]
    fn data_sets_body_and_form_headers() -> Result<(), ProtocolError> {
        let request = Request::new("https://example.com/form", "POST")?
            .data(vec![("foo", "bar"), ("space", "a b")]);

        let body = request.body.as_ref().expect("body is set for form data");
        assert_eq!(body.as_ref(), b"foo=bar&space=a+b");
        assert!(matches!(request.data, Some(FormBody::Fields(_))));
        assert!(request.json.is_none());

        let headers = request.prepare_headers();
        assert_eq!(
            header_value(&headers, CONTENT_TYPE_HEADER),
            Some(APPLICATION_X_WWW_FORM_URLENCODED)
        );
        assert!(header_value(&headers, "user-agent").is_some());
        Ok(())
    }

    #[test]
    fn json_sets_body_and_content_type() -> Result<(), ProtocolError> {
        let payload = json!({"message": "hello"});
        let request = Request::new("https://example.com/json", "POST")?.json(payload.clone());

        assert!(request.data.is_none());
        assert!(request.json.is_some());
        let expected = serde_json::to_vec(&payload).unwrap();
        let body = request.body.as_ref().expect("body is set for json");
        assert_eq!(body.as_ref(), expected.as_slice());

        let headers = request.prepare_headers();
        assert_eq!(
            header_value(&headers, CONTENT_TYPE_HEADER),
            Some(APPLICATION_JSON)
        );
        assert!(header_value(&headers, "user-agent").is_some());
        Ok(())
    }

    #[test]
    fn prepare_headers_includes_cookies_when_missing() -> Result<(), ProtocolError> {
        let request = Request::new("https://example.com", "GET")?
            .cookies(vec![("session", "abc"), ("theme", "dark")]);

        let headers = request.prepare_headers();
        assert_eq!(
            header_value(&headers, COOKIE_HEADER),
            Some("session=abc; theme=dark")
        );
        assert!(header_value(&headers, "user-agent").is_some());
        Ok(())
    }

    #[test]
    fn prepare_headers_respects_existing_values() -> Result<(), ProtocolError> {
        let request = Request::new("https://example.com", "POST")?
            .header("Content-Type: text/plain")
            .header("User-Agent: custom-agent")
            .header("Cookie: manual=1");

        let headers = request.prepare_headers();
        assert_eq!(header_count(&headers, "content-type"), 1);
        assert_eq!(header_value(&headers, "content-type"), Some("text/plain"));
        assert_eq!(header_count(&headers, "user-agent"), 1);
        assert_eq!(header_value(&headers, "user-agent"), Some("custom-agent"));
        assert_eq!(header_count(&headers, "cookie"), 1);
        assert_eq!(header_value(&headers, "cookie"), Some("manual=1"));
        Ok(())
    }

    #[test]
    fn prepare_pseudo_headers_for_regular_request() -> Result<(), ProtocolError> {
        let request = Request::new("https://example.com/path", "GET")?;
        let pseudo = Request::prepare_pseudo_headers(&request)?;

        assert_eq!(header_value(&pseudo, ":method"), Some("GET"));
        assert_eq!(header_value(&pseudo, ":path"), Some("/path"));
        assert_eq!(header_value(&pseudo, ":scheme"), Some("https"));
        assert_eq!(header_value(&pseudo, ":authority"), Some("example.com"));
        Ok(())
    }

    #[test]
    fn prepare_pseudo_headers_for_connect_request() -> Result<(), ProtocolError> {
        let request = Request::new("https://example.com:8443", "CONNECT")?;
        let pseudo = Request::prepare_pseudo_headers(&request)?;

        assert_eq!(header_value(&pseudo, ":method"), Some("CONNECT"));
        assert_eq!(header_value(&pseudo, ":authority"), Some("example.com:8443"));
        assert!(header_value(&pseudo, ":scheme").is_none());
        assert!(header_value(&pseudo, ":path").is_none());
        Ok(())
    }

    #[test]
    fn prepare_request_produces_consistent_structure() -> Result<(), ProtocolError> {
        let request = Request::new("https://example.com/api", "POST")?
            .params(vec![("k", "v")])
            .cookies(vec![("session", "abc")])
            .data(vec![("foo", "bar")])
            .trailers(vec!["checksum: 123".to_string()]);

        let prepared = request.prepare_request()?;
        assert_eq!(prepared.method, "POST");
        assert_eq!(prepared.path, "/api?k=v");
        assert!(header_value(&prepared.pseudo_headers, ":method").is_some());
        assert!(header_value(&prepared.pseudo_headers, ":path").is_some());
        assert!(header_value(&prepared.pseudo_headers, ":scheme").is_some());
        assert!(header_value(&prepared.pseudo_headers, ":authority").is_some());
        assert_eq!(
            prepared.body.as_ref().map(|b| b.as_ref()),
            Some(b"foo=bar".as_ref())
        );
        assert_eq!(prepared.trailers.len(), 1);
        assert_eq!(
            header_value(&prepared.headers, COOKIE_HEADER),
            Some("session=abc")
        );
        assert_eq!(
            header_value(&prepared.headers, CONTENT_TYPE_HEADER),
            Some(APPLICATION_X_WWW_FORM_URLENCODED)
        );
        Ok(())
    }
}
