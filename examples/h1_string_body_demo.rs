use riphttplib::h1::H1Client;
use riphttplib::types::{Header, Protocol};
use riphttplib::utils::parse_target;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("HTTP/1.1 String Body Demo");

    let client = H1Client::new();
    let target = parse_target("http://httpbin.org/post");

    let headers = vec![
        Header::new("Content-Type".to_string(), "text/plain".to_string()),
        Header::new("X-Demo".to_string(), "string-body-test".to_string()),
    ];

    // Test with &str body
    println!("\n🔄 Testing POST with &str body...");
    let str_body = "Hello from string slice!";

    match client
        .post_str(&target, Some(headers.clone()), Some(str_body), None)
        .await
    {
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

    match client
        .post_string(
            &target,
            Some(headers.clone()),
            Some(string_body.clone()),
            None,
        )
        .await
    {
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
    let get_target = parse_target("http://httpbin.org/get");

    match client
        .get_str(&get_target, None, Some("GET request with body"), None)
        .await
    {
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

    match client
        .send_str(
            &target,
            Some("PUT".to_string()),
            Some(headers),
            Some("PUT body content"),
            None,
        )
        .await
    {
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
