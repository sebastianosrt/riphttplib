use riphttplib::h1::H1Client;
use riphttplib::types::{ProxySettings, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = H1Client::new();

    let _proxy_settings = ProxySettings::parse(
        Some("http://localhost:8081"),
        Some("http://localhost:8081"),
        Some("socks5://127.0.0.1:9050")
    )?;

    // Example 1: HTTP Proxy
    println!("=== 1. HTTP Proxy Example ===");
    let http_request = Request::new("http://httpbin.org/ip", "GET")?
        .with_http_proxy("http://localhost:8081")?;

    match client.send_request(http_request).await {
        Ok(response) => {
            println!("✓ HTTP Proxy - Status: {}", response.status);
            println!("  Response: {}", String::from_utf8_lossy(&response.body));
        }
        Err(e) => {
            println!("✗ HTTP Proxy failed: {}", e);
        }
    }

    // Example 2: HTTPS Proxy
    println!("\n=== 2. HTTPS Proxy Example ===");
    let https_request = Request::new("https://httpbin.org/ip", "GET")?
        .with_https_proxy("http://localhost:8081")?;

    match client.send_request(https_request).await {
        Ok(response) => {
            println!("✓ HTTPS Proxy - Status: {}", response.status);
            println!("  Response: {}", String::from_utf8_lossy(&response.body));
        }
        Err(e) => {
            println!("✗ HTTPS Proxy failed: {}", e);
        }
    }

    // Example 3: SOCKS5 Proxy (like Tor)
    println!("\n=== 3. SOCKS5 Proxy Example ===");
    let socks5_request = Request::new("https://httpbin.org/ip", "GET")?
        .with_socks5_proxy("socks5://127.0.0.1:9050")?;

    match client.send_request(socks5_request).await {
        Ok(response) => {
            println!("✓ SOCKS5 Proxy - Status: {}", response.status);
            println!("  Response: {}", String::from_utf8_lossy(&response.body));
        }
        Err(e) => {
            println!("✗ SOCKS5 Proxy failed: {}", e);
        }
    }

    // Example 4: SOCKS5 Proxy with Authentication
    println!("\n=== 4. SOCKS5 Proxy with Auth Example ===");
    let socks5_auth_request = Request::new("https://httpbin.org/ip", "GET")?
        .with_socks5_proxy_auth(
            "socks5://localhost:9050",
            "username".to_string(),
            "password".to_string()
        )?;

    match client.send_request(socks5_auth_request).await {
        Ok(response) => {
            println!("✓ SOCKS5 Auth - Status: {}", response.status);
            println!("  Response: {}", String::from_utf8_lossy(&response.body));
        }
        Err(e) => {
            println!("✗ SOCKS5 Auth failed: {}", e);
        }
    }

    // Example 5: SOCKS4 Proxy
    println!("\n=== 5. SOCKS4 Proxy Example ===");
    let socks4_request = Request::new("https://httpbin.org/ip", "GET")?
        .with_socks4_proxy("socks4://localhost:9050")?;

    match client.send_request(socks4_request).await {
        Ok(response) => {
            println!("✓ SOCKS4 Proxy - Status: {}", response.status);
            println!("  Response: {}", String::from_utf8_lossy(&response.body));
        }
        Err(e) => {
            println!("✗ SOCKS4 Proxy failed: {}", e);
        }
    }

    // Example 7: Direct Connection (No Proxy)
    println!("\n=== 7. Direct Connection (No Proxy) ===");
    let direct_request = Request::new("https://httpbin.org/ip", "GET")?;

    match client.send_request(direct_request).await {
        Ok(response) => {
            println!("✓ Direct - Status: {}", response.status);
            println!("  Response: {}", String::from_utf8_lossy(&response.body));
        }
        Err(e) => {
            println!("✗ Direct connection failed: {}", e);
        }
    }

    println!("\n=== Proxy Examples Complete ===");
    println!("To test with real proxies:");
    println!("  • HTTP: Install mitmproxy and run: mitmproxy -p 8080");
    println!("  • SOCKS5: Install Tor and run: tor");
    println!("  • SOCKS4: Use SSH tunnel: ssh -D 1080 user@host");

    Ok(())
}