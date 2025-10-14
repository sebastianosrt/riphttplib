use bytes::Bytes;
use riphttplib::h1::H1Client;
use riphttplib::types::{Header, Request};
use riphttplib::utils::parse_target;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("HTTP/1.1 String Body Demo");

    let client = H1Client::new();
    let target = parse_target("http://httpbin.org/post")?;

    let headers = vec![
        Header::new("Content-Type".to_string(), "text/plain".to_string()),
        Header::new("X-Demo".to_string(), "string-body-test".to_string()),
    ];

    // Test with &str body
    println!("\n🔄 Testing POST with &str body...");
    let str_body = "Hello from string slice!";

    let request = Request::new("POST")
        .with_headers(headers.clone())
        .with_body(Bytes::from(str_body.to_owned()))
        .with_trailers(None);

    match client.send_request(&target, request).await {
        Ok(response) => {
            println!("✅ &str body request successful!");
            println!("   Status: {}", response.status);
            println!("   Body size: {} bytes", response.body.len());
        }
        Err(e) => {
            println!("❌ &str body request failed: {}", e);
        }
    }

    // Test with String body
    println!("\n🔄 Testing POST with String body...");
    let string_body = format!(
        "Hello from String! Timestamp: {}",
        chrono::Utc::now().to_rfc3339()
    );

    let request = Request::new("POST")
        .with_headers(headers.clone())
        .with_body(Bytes::from(string_body.clone()))
        .with_trailers(None);

    match client.send_request(&target, request).await {
        Ok(response) => {
            println!("✅ String body request successful!");
            println!("   Status: {}", response.status);
            println!("   Body size: {} bytes", response.body.len());

            // Try to parse JSON response to verify our body was sent
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                if let Some(data) = json_value.get("data") {
                    println!("   Sent body: {}", data);
                }
            }
        }
        Err(e) => {
            println!("❌ String body request failed: {}", e);
        }
    }

    // Test GET with string body (unusual but supported)
    println!("\n🔄 Testing GET with string body...");
    let get_target = parse_target("http://httpbin.org/get")?;

    let get_request = Request::new("GET")
        .with_optional_body(Some(Bytes::from("GET request with body".to_string())));
    match client.send_request(&get_target, get_request).await {
        Ok(response) => {
            println!("✅ GET with string body successful!");
            println!("   Status: {}", response.status);
        }
        Err(e) => {
            println!("❌ GET with string body failed: {}", e);
        }
    }

    // Test send_str with custom method
    println!("\n🔄 Testing custom method with string body...");

    let put_request = Request::new("PUT")
        .with_headers(headers)
        .with_body(Bytes::from("PUT body content".to_string()));

    match client.send_request(&target, put_request).await {
        Ok(response) => {
            println!("✅ Custom method (PUT) with string body successful!");
            println!("   Status: {}", response.status);
        }
        Err(e) => {
            println!("❌ Custom method failed: {}", e);
        }
    }

    println!("\n✨ String body demo completed!");
    Ok(())
}
