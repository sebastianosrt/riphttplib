use riphttplib::h1::H1Client;
use riphttplib::types::{ProxySettings, Request};
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Note: This example requires a proxy server to be running
    // You can use tools like mitmproxy, squid, or tinyproxy for testing

    println!("=== Proxy Configuration Example ===");

    // Configure proxy settings
    let proxy_settings = ProxySettings {
        http: Some(Url::parse("http://localhost:8080")?),
        https: Some(Url::parse("http://localhost:8080")?),
    };

    // Create a request with proxy configuration
    let request = Request::new("http://httpbin.org/ip", "GET")?
        .with_proxies(proxy_settings);

    println!("Making request through proxy...");

    let client = H1Client::new();

    // This will attempt to connect through the proxy
    match client.send_request(request).await {
        Ok(response) => {
            println!("Status: {}", response.status);
            println!("Response: {}", String::from_utf8_lossy(&response.body));
            println!("Request successful through proxy!");
        }
        Err(e) => {
            eprintln!("Proxy request failed: {}", e);
            eprintln!("Make sure you have a proxy running on localhost:8080");
            eprintln!("Or modify the proxy URL in this example to match your setup");
        }
    }

    // Example without proxy for comparison
    println!("\n=== Direct Connection (No Proxy) ===");
    let direct_request = Request::new("http://httpbin.org/ip", "GET")?
        .without_proxies();

    match client.send_request(direct_request).await {
        Ok(response) => {
            println!("Status: {}", response.status);
            println!("Response: {}", String::from_utf8_lossy(&response.body));
            println!("Direct request successful!");
        }
        Err(e) => {
            eprintln!("Direct request failed: {}", e);
        }
    }

    Ok(())
}