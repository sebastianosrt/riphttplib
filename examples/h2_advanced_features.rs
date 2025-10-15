use riphttplib::h2::H2Client;
use riphttplib::types::{ClientTimeouts, Header, Request};
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== HTTP/2 Advanced Features Demo ===");

    // Create an H2 client with custom timeouts
    let client_timeouts = ClientTimeouts {
        connect: Some(Duration::from_secs(5)),
        read: Some(Duration::from_secs(15)),
        write: Some(Duration::from_secs(5)),
    };

    let client = H2Client::with_timeouts(client_timeouts);

    // Example 1: JSON POST with parameters and cookies over HTTP/2
    println!("\n--- JSON POST with params and cookies ---");
    let request = Request::new("https://httpbin.org/anything", "POST")?
        .with_header(Header::new("X-Protocol".to_string(), "HTTP/2".to_string()))
        .with_params(vec![
            ("source", "riphttplib-h2"),
            ("test", "advanced-features"),
        ])
        .with_json(json!({
            "message": "Hello from HTTP/2 client!",
            "features": ["params", "json", "cookies", "timeouts"],
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
        .with_cookies(vec![
            ("h2_session", "session123"),
            ("client_id", "riphttplib"),
        ])
        .with_allow_redirects(true)
        .with_stream(false);

    match client.send_request(request).await {
        Ok(response) => {
            println!("✓ Status: {}", response.status);
            println!("✓ Protocol: {}", response.protocol_version);
            println!("✓ Response length: {} bytes", response.body.len());

            // Show a preview of the response
            let response_text = String::from_utf8_lossy(&response.body);
            if response_text.len() > 300 {
                println!("✓ Response preview: {}...", &response_text[..300]);
            } else {
                println!("✓ Response: {}", response_text);
            }
        }
        Err(e) => {
            eprintln!("✗ Request failed: {}", e);
        }
    }

    // Example 2: Streaming response
    println!("\n--- Streaming response ---");
    let streaming_request = Request::new("https://httpbin.org/stream/3", "GET")?
        .with_header(Header::new("Accept".to_string(), "application/json".to_string()))
        .with_stream(true);

    match client.send_request(streaming_request).await {
        Ok(response) => {
            println!("✓ Streaming Status: {}", response.status);
            println!("✓ Partial response length: {} bytes", response.body.len());
            println!("✓ First chunk: {}", String::from_utf8_lossy(&response.body));
        }
        Err(e) => {
            eprintln!("✗ Streaming request failed: {}", e);
        }
    }

    // Example 3: Request with trailers
    println!("\n--- Request with trailers ---");
    let trailers = vec![
        Header::new("x-checksum".to_string(), "sha256:abcd1234".to_string()),
        Header::new("x-request-id".to_string(), "req-789".to_string()),
    ];

    let request_with_trailers = Request::new("https://httpbin.org/post", "POST")?
        .with_body("Request body with trailers")
        .with_trailers(Some(trailers))
        .with_header(Header::new("Content-Type".to_string(), "text/plain".to_string()));

    match client.send_request(request_with_trailers).await {
        Ok(response) => {
            println!("✓ Trailers Status: {}", response.status);
            if let Some(resp_trailers) = &response.trailers {
                println!("✓ Response has {} trailers", resp_trailers.len());
            }
        }
        Err(e) => {
            eprintln!("✗ Trailers request failed: {}", e);
        }
    }

    // Example 4: Redirect handling
    println!("\n--- Redirect handling ---");

    // First, disable redirects to see the redirect response
    let no_redirect_req = Request::new("https://httpbin.org/redirect/1", "GET")?
        .with_allow_redirects(false);

    match client.send_request(no_redirect_req).await {
        Ok(response) => {
            println!("✓ No-redirect Status: {} (redirect response captured)", response.status);
        }
        Err(e) => {
            eprintln!("✗ No-redirect request failed: {}", e);
        }
    }

    // Then enable redirects (default behavior)
    let redirect_req = Request::new("https://httpbin.org/redirect/1", "GET")?
        .with_allow_redirects(true);

    match client.send_request(redirect_req).await {
        Ok(response) => {
            println!("✓ With-redirect Status: {} (followed redirect)", response.status);
        }
        Err(e) => {
            eprintln!("✗ Redirect request failed: {}", e);
        }
    }

    println!("\n🎉 HTTP/2 advanced features demo completed!");
    Ok(())
}