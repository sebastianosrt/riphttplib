use bytes::Bytes;
use riphttplib::h2::H2Client;
use riphttplib::types::{Header, Request};
use riphttplib::utils::parse_target;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the default crypto provider for rustls
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    println!("HTTP/2 Basic Demo");

    let client = H2Client::new();
    // let target = parse_target("https://httpbin.org/post");
    let target = parse_target("https://localhost:51443")?;

    let headers = vec![Header::new("aaaaa".to_string(), "aaaaaaaaa".to_string())];
    let trailers = vec![Header::new("host".to_string(), "xxxxxxxx".to_string())];

    // Test basic GET request
    println!("\n🔄 Testing HTTP/2 request...");

    let request = Request::new("POST")
        .with_headers(headers)
        .with_body(Bytes::from("aaaaaaaa".to_string()))
        .with_trailers(Some(trailers));

    match client.send_request(&target, request).await {
        Ok(response) => {
            println!("✅ HTTP/2 GET request successful!");
            println!("   Status: {}", response.status);
            println!("   Protocol: {}", response.protocol_version);
            println!("   Headers: {}", response.headers.len());
            println!("   Body size: {} bytes", response.body.len());

            // Print some response headers
            for header in &response.headers {
                println!("   Header: {}: {:?}", header.name, header.value);
            }

            println!("   Body: {}", String::from_utf8_lossy(&response.body));
        }
        Err(e) => {
            println!("❌ HTTP/2 GET request failed: {}", e);
            println!("   This might be due to server settings or our HTTP/2 implementation");
        }
    }

    println!("\n✨ HTTP/2 demo completed!");
    Ok(())
}
