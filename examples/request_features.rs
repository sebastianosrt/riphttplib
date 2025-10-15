use riphttplib::h1::H1Client;
use riphttplib::types::{ClientTimeouts, Header, Request};
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example 1: Query parameters
    println!("=== Example 1: Query Parameters ===");
    let request = Request::new("https://httpbin.org/get", "GET")?
        .with_params(vec![
            ("search", "rust http client"),
            ("page", "1"),
            ("limit", "10"),
        ]);

    let client = H1Client::new();
    let response = client.send_request(request).await?;
    println!("Status: {}", response.status);
    let default_content_type = "unknown".to_string();
    let content_type = response.headers.iter()
        .find(|h| h.name == "content-type")
        .and_then(|h| h.value.as_ref())
        .unwrap_or(&default_content_type);
    println!("Content-Type: {}", content_type);

    // Example 2: JSON body
    println!("\n=== Example 2: JSON Body ===");
    let json_data = json!({
        "user": "alice",
        "action": "login",
        "timestamp": "2024-01-01T00:00:00Z",
        "metadata": {
            "client": "riphttplib",
            "version": "0.1.0"
        }
    });

    let request = Request::new("https://httpbin.org/post", "POST")?
        .with_json(json_data);

    let response = client.send_request(request).await?;
    println!("Status: {}", response.status);
    println!("Response body length: {} bytes", response.body.len());

    // Example 3: Cookies
    println!("\n=== Example 3: Cookies ===");
    let request = Request::new("https://httpbin.org/cookies", "GET")?
        .with_cookies(vec![
            ("session_id", "abc123xyz"),
            ("user_preference", "dark_mode"),
            ("language", "en-US"),
        ]);

    let response = client.send_request(request).await?;
    println!("Status: {}", response.status);
    println!("Cookies were sent successfully");

    // Example 4: Custom timeouts
    println!("\n=== Example 4: Custom Timeouts ===");
    let custom_timeouts = ClientTimeouts {
        connect: Some(Duration::from_secs(5)),
        read: Some(Duration::from_secs(10)),
        write: Some(Duration::from_secs(5)),
    };

    let request = Request::new("https://httpbin.org/delay/2", "GET")?
        .with_timeout(custom_timeouts);

    let start = std::time::Instant::now();
    let response = client.send_request(request).await?;
    let elapsed = start.elapsed();

    println!("Status: {}", response.status);
    println!("Request completed in: {:?}", elapsed);

    // Example 5: Disable redirects
    println!("\n=== Example 5: Redirect Control ===");
    let request = Request::new("https://httpbin.org/redirect/3", "GET")?
        .with_allow_redirects(false);

    let response = client.send_request(request).await?;
    println!("Status: {} (redirect not followed)", response.status);

    // Now with redirects enabled (default)
    let request = Request::new("https://httpbin.org/redirect/1", "GET")?
        .with_allow_redirects(true);

    let response = client.send_request(request).await?;
    println!("Status: {} (redirect followed)", response.status);

    // Example 6: Streaming response
    println!("\n=== Example 6: Streaming Response ===");
    let request = Request::new("https://httpbin.org/stream/5", "GET")?
        .with_stream(true);

    let response = client.send_request(request).await?;
    println!("Status: {}", response.status);
    println!("Streamed response body length: {} bytes (partial)", response.body.len());
    println!("Body preview: {}", String::from_utf8_lossy(&response.body[..response.body.len().min(200)]));

    // Example 7: Complete feature combination
    println!("\n=== Example 7: Combined Features ===");
    let complex_request = Request::new("https://httpbin.org/anything", "POST")?
        .with_header(Header::new("X-Custom-Header".to_string(), "example-value".to_string()))
        .with_params(vec![("source", "riphttplib-example")])
        .with_json(json!({"message": "Hello from riphttplib!"}))
        .with_cookies(vec![("example_cookie", "cookie_value")])
        .with_timeout(ClientTimeouts {
            connect: Some(Duration::from_secs(10)),
            read: Some(Duration::from_secs(30)),
            write: Some(Duration::from_secs(10)),
        })
        .with_allow_redirects(true)
        .with_stream(false);

    let response = client.send_request(complex_request).await?;
    println!("Status: {}", response.status);
    println!("Combined features request completed successfully!");

    Ok(())
}