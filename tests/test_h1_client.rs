use bytes::Bytes;
use riphttplib::h1::H1Client;
use riphttplib::types::{Header, Protocol, ProtocolError, Target};
use riphttplib::utils::parse_target;
use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_target() -> Target {
        Target {
            host: "httpbin.org".to_string(),
            port: 80,
            url: "http://httpbin.org/get".to_string(),
            protocols: HashSet::new(),
            scheme: "http".to_string(),
            path: "/get".to_string(),
        }
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
        let target = create_test_target();

        let result = client.get(&target, None, None, None).await;

        match result {
            Ok(response) => {
                assert_eq!(response.status, 200);
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

        let result = client.get(&target, Some(headers), None, None).await;

        match result {
            Ok(response) => {
                assert_eq!(response.status, 200);
                assert!(response.protocol_version.starts_with("HTTP/"));
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
        let mut target = create_test_target();
        target.path = "/post".to_string();
        target.url = "http://httpbin.org/post".to_string();

        let body = Bytes::from(r#"{"test": "data"}"#);
        let headers = vec![Header::new(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )];

        let result = client.post(&target, Some(headers), Some(body), None).await;

        match result {
            Ok(response) => {
                assert_eq!(response.status, 200);
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

        let result = client
            .send(&target, Some("HEAD".to_string()), None, None, None)
            .await;

        match result {
            Ok(response) => {
                assert_eq!(response.status, 200);
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
        let target = parse_target("http://example.com/test?param=value");

        assert_eq!(target.host, "example.com");
        assert_eq!(target.port, 80);
        assert_eq!(target.scheme, "http");
        assert_eq!(target.path, "/test");
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

        let response_with_protocol =
            riphttplib::types::Response::new_with_protocol(404, "HTTP/1.0".to_string());
        assert_eq!(response_with_protocol.status, 404);
        assert_eq!(response_with_protocol.protocol_version, "HTTP/1.0");
    }

    #[tokio::test]
    async fn test_error_cases() {
        let client = H1Client::new();

        // Test with invalid target (should fail to connect)
        let invalid_target = Target {
            host: "invalid.nonexistent.domain.test".to_string(),
            port: 80,
            url: "http://invalid.nonexistent.domain.test/".to_string(),
            protocols: HashSet::new(),
            scheme: "http".to_string(),
            path: "/".to_string(),
        };

        let result = client.get(&invalid_target, None, None, None).await;
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
