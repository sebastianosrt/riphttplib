use riphttplib::h3::H3Client;
use riphttplib::types::{ClientTimeouts, Header, Request};
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== HTTP/3 Advanced Features Demo ===");

    // Create an H3 client with custom timeouts
    let client_timeouts = ClientTimeouts {
        connect: Some(Duration::from_secs(10)), // H3 might need more time for QUIC handshake
        read: Some(Duration::from_secs(30)),
        write: Some(Duration::from_secs(10)),
    };

    let client = H3Client::with_timeouts(client_timeouts);

    // Example 1: Complex request with all new features
    println!("\n--- Complex HTTP/3 request ---");
    let complex_request = Request::new("https://quic.tech:8443/", "POST")?
        .with_header(Header::new("X-Protocol".to_string(), "HTTP/3".to_string()))
        .with_header(Header::new("User-Agent".to_string(), "riphttplib-h3-demo".to_string()))
        .with_params(vec![
            ("test", "h3-features"),
            ("client", "riphttplib"),
            ("version", "0.1.0"),
        ])
        .with_json(json!({
            "protocol": "HTTP/3",
            "transport": "QUIC",
            "features": {
                "multiplexing": true,
                "header_compression": "QPACK",
                "0rtt": false
            },
            "message": "Testing HTTP/3 with riphttplib!"
        }))
        .with_cookies(vec![
            ("h3_session", "quic-session-456"),
            ("performance_mode", "optimized"),
        ])
        .with_timeout(ClientTimeouts {
            connect: Some(Duration::from_secs(15)),
            read: Some(Duration::from_secs(45)),
            write: Some(Duration::from_secs(15)),
        })
        .with_allow_redirects(true)
        .with_stream(false);

    match client.send_request(complex_request).await {
        Ok(response) => {
            println!("✓ Status: {}", response.status);
            println!("✓ Protocol: {}", response.protocol_version);
            println!("✓ Headers count: {}", response.headers.len());
            println!("✓ Response length: {} bytes", response.body.len());

            // Show response headers
            for header in &response.headers {
                if header.name.starts_with("alt-svc") || header.name.contains("quic") {
                    println!("✓ {}: {:?}", header.name, header.value);
                }
            }

            // Show response preview
            let response_text = String::from_utf8_lossy(&response.body);
            if response_text.len() > 200 {
                println!("✓ Response preview: {}...", &response_text[..200]);
            } else {
                println!("✓ Response: {}", response_text);
            }
        }
        Err(e) => {
            eprintln!("✗ HTTP/3 request failed: {}", e);
            eprintln!("  Note: This might fail if the server doesn't support HTTP/3");
            eprintln!("  or if there are network connectivity issues with QUIC");
        }
    }

    // Example 2: Streaming over HTTP/3
    println!("\n--- HTTP/3 streaming ---");
    let streaming_request = Request::new("https://quic.tech:8443/", "GET")?
        .with_header(Header::new("Accept".to_string(), "text/plain".to_string()))
        .with_params(vec![("stream", "true"), ("chunks", "5")])
        .with_stream(true);

    match client.send_request(streaming_request).await {
        Ok(response) => {
            println!("✓ Streaming Status: {}", response.status);
            println!("✓ First chunk length: {} bytes", response.body.len());

            if !response.body.is_empty() {
                println!("✓ First chunk: {}", String::from_utf8_lossy(&response.body));
            }
        }
        Err(e) => {
            eprintln!("✗ HTTP/3 streaming failed: {}", e);
        }
    }

    // Example 3: Request with trailers over HTTP/3
    println!("\n--- HTTP/3 with trailers ---");
    let trailers = vec![
        Header::new("x-quic-stream-id".to_string(), "stream-123".to_string()),
        Header::new("x-transmission-time".to_string(), "optimal".to_string()),
    ];

    let trailers_request = Request::new("https://quic.tech:8443/", "POST")?
        .with_body("HTTP/3 request body with QUIC transport")
        .with_trailers(Some(trailers))
        .with_header(Header::new("Content-Type".to_string(), "text/plain".to_string()))
        .with_cookies(vec![("quic_test", "trailers_demo")]);

    match client.send_request(trailers_request).await {
        Ok(response) => {
            println!("✓ Trailers Status: {}", response.status);
            if let Some(resp_trailers) = &response.trailers {
                println!("✓ Response trailers count: {}", resp_trailers.len());
                for trailer in resp_trailers {
                    println!("  - {}: {:?}", trailer.name, trailer.value);
                }
            }
        }
        Err(e) => {
            eprintln!("✗ HTTP/3 trailers request failed: {}", e);
        }
    }

    // Example 4: Fallback demonstration (when H3 is not available)
    println!("\n--- Fallback behavior ---");
    println!("Note: In production, you might want to implement fallback logic:");
    println!("1. Try HTTP/3 first");
    println!("2. Fall back to HTTP/2 if HTTP/3 fails");
    println!("3. Fall back to HTTP/1.1 as last resort");

    println!("\n🚀 HTTP/3 advanced features demo completed!");
    println!("Remember: HTTP/3 support depends on server configuration and network conditions.");

    Ok(())
}