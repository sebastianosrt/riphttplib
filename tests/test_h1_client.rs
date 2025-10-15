use bytes::Bytes;
use riphttplib::h1::H1Client;
use riphttplib::types::{Header, ProtocolError, Request, Target};
use riphttplib::utils::parse_target;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_target() -> Target {
        parse_target("http://httpbin.org/get").expect("valid test target")
    }

    #[tokio::test]
    async fn test_h1_client_creation() {
        let _client = H1Client::new();
        // Just verify we can create a client without panicking
    }

    #[tokio::test]
    async fn test_parse_status_line() {
        // Test valid status line
        let status_line = "HTTP/1.1 200 OK\r\n";
        let result = H1Client::parse_status_line(status_line);
        assert!(result.is_ok());
        let (status, protocol) = result.unwrap();
        assert_eq!(status, 200);
        assert_eq!(protocol, "HTTP/1.1");

        // Test HTTP/1.0 response
        let status_line = "HTTP/1.0 404 Not Found\r\n";
        let result = H1Client::parse_status_line(status_line);
        assert!(result.is_ok());
        let (status, protocol) = result.unwrap();
        assert_eq!(status, 404);
        assert_eq!(protocol, "HTTP/1.0");

        // Test invalid status line
        let status_line = "Invalid";
        let result = H1Client::parse_status_line(status_line);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_h1_get_request_basic() {
        let client = H1Client::new();

        let request = Request::new("http://httpbin.org/get", "GET").expect("valid request");
        let result = client.send_request(request).await;

        match result {
            Ok(response) => {
                // Accept either 200 or 503 since httpbin.org can be unreliable
                assert!(response.status == 200 || response.status == 503);
                assert!(response.protocol_version.starts_with("HTTP/"));
                assert!(!response.headers.is_empty());
                assert!(!response.body.is_empty());
            }
            Err(e) => {
                // Network tests might fail, so we'll just verify the error type is reasonable
                match e {
                    ProtocolError::ConnectionFailed(_) | ProtocolError::Io(_) => {
                        // These are acceptable failures for network tests
                        println!("Network test failed (expected in some environments): {}", e);
                    }
                    _ => panic!("Unexpected error type: {:?}", e),
                }
            }
        }
    }

    #[tokio::test]
    async fn test_h1_get_with_headers() {
        let client = H1Client::new();
        let target = create_test_target();

        let headers = vec![
            Header::new("Accept".to_string(), "application/json".to_string()),
            Header::new("X-Test-Header".to_string(), "test-value".to_string()),
        ];

        let request = Request::with_target(target, "GET").with_headers(headers);
        let result = client.send_request(request).await;

        match result {
            Ok(response) => {
                if !(200..400).contains(&response.status) {
                    println!(
                        "Non-success status returned during test ({}), headers {:?}",
                        response.status, response.headers
                    );
                } else {
                    assert!(response.protocol_version.starts_with("HTTP/"));
                }
            }
            Err(e) => {
                // Network failure is acceptable
                match e {
                    ProtocolError::ConnectionFailed(_) | ProtocolError::Io(_) => {
                        println!("Network test failed (expected): {}", e);
                    }
                    _ => panic!("Unexpected error: {:?}", e),
                }
            }
        }
    }

    #[tokio::test]
    async fn test_h1_post_with_body() {
        let client = H1Client::new();
        let target = parse_target("http://httpbin.org/post").expect("valid post target");

        let body = Bytes::from(r#"{"test": "data"}"#);
        let headers = vec![Header::new(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )];

        let request = Request::with_target(target, "POST")
            .with_headers(headers)
            .with_body(body.clone());
        let result = client.send_request(request).await;

        match result {
            Ok(response) => {
                assert!(response.status == 200 || response.status == 503);
                assert!(response.protocol_version.starts_with("HTTP/"));
            }
            Err(e) => match e {
                ProtocolError::ConnectionFailed(_) | ProtocolError::Io(_) => {
                    println!("Network test failed (expected): {}", e);
                }
                _ => panic!("Unexpected error: {:?}", e),
            },
        }
    }

    #[tokio::test]
    async fn test_h1_custom_method() {
        let client = H1Client::new();
        let target = create_test_target();

        let request = Request::with_target(target, "HEAD");
        let result = client.send_request(request).await;

        match result {
            Ok(response) => {
                assert!(response.status == 200 || response.status == 503);
                assert!(response.protocol_version.starts_with("HTTP/"));
                // HEAD responses should have empty body according to RFC 7231
                assert!(
                    response.body.is_empty(),
                    "HEAD response should have empty body"
                );
            }
            Err(e) => match e {
                ProtocolError::ConnectionFailed(_) | ProtocolError::Io(_) => {
                    println!("Network test failed (expected): {}", e);
                }
                _ => panic!("Unexpected error: {:?}", e),
            },
        }
    }

    #[tokio::test]
    async fn test_parse_target_integration() {
        let target = parse_target("http://example.com/test?param=value").expect("valid url");

        assert_eq!(target.host().unwrap(), "example.com");
        assert_eq!(target.port().unwrap(), 80);
        assert_eq!(target.scheme(), "http");
        assert_eq!(target.path_only(), "/test");
    }

    #[tokio::test]
    async fn test_header_creation() {
        let header = Header::new("Content-Type".to_string(), "application/json".to_string());
        assert_eq!(header.name, "Content-Type");
        assert_eq!(header.value, Some("application/json".to_string()));

        let header_str = header.to_string();
        assert_eq!(header_str, "Content-Type: application/json");

        let valueless = Header::new_valueless("Authorization".to_string());
        assert_eq!(valueless.name, "Authorization");
        assert_eq!(valueless.value, None);
        assert_eq!(valueless.to_string(), "Authorization");
    }

    #[test]
    fn test_response_creation() {
        let response = riphttplib::types::Response::new(200);
        assert_eq!(response.status, 200);
        assert_eq!(response.protocol_version, "HTTP/1.1");
        assert!(response.headers.is_empty());
        assert!(response.body.is_empty());
        assert!(response.trailers.is_none());

        let mut response_with_protocol = riphttplib::types::Response::new(404);
        response_with_protocol.protocol_version = "HTTP/1.0".to_string();
        assert_eq!(response_with_protocol.status, 404);
        assert_eq!(response_with_protocol.protocol_version, "HTTP/1.0");
    }

    #[tokio::test]
    async fn test_error_cases() {
        let client = H1Client::new();

        // Test with invalid target (should fail to connect)
        let invalid_target =
            parse_target("http://invalid.nonexistent.domain.test/").expect("valid format target");

        let request = Request::with_target(invalid_target, "GET");
        let result = client.send_request(request).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            ProtocolError::ConnectionFailed(_) => {
                // Expected error type
            }
            ProtocolError::Io(_) => {
                // Also acceptable
            }
            other => panic!("Unexpected error type: {:?}", other),
        }
    }
}
