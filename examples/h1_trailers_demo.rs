use bytes::Bytes;
use riphttplib::h1::H1Client;
use riphttplib::types::{Header, Protocol};
use riphttplib::utils::parse_target;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("HTTP/1.1 Client");

    // Create H1 client
    let client = H1Client::new();

    // Setup target for POST request
    let target = parse_target("http://httpbin.org/post");

    // Print target information
    println!("\nTarget Information:");
    println!("   Host: {}", target.host);
    println!("   Port: {}", target.port);
    println!("   Scheme: {}", target.scheme);
    println!("   Path: {}", target.path);
    println!("   URL: {}", target.url);

    // Create headers
    let headers = vec![
        Header::new("Content-Type".to_string(), "application/json".to_string()),
        Header::new("X-Custom-Header".to_string(), "demo-value".to_string()),
        Header::new("Accept".to_string(), "*/*".to_string()),
        Header::new("User-Agent".to_string(), "RipHTTPLib-Demo/1.0".to_string()),
    ];

    // Create trailers (these will be sent after the body in chunked encoding)
    let trailers = vec![
        Header::new(
            "X-Content-Checksum".to_string(),
            "sha256:abc123".to_string(),
        ),
        Header::new("X-Processing-Time".to_string(), "150ms".to_string()),
        Header::new("X-Server-ID".to_string(), "server-01".to_string()),
    ];

    // Create request body
    let request_body = serde_json::json!({
        "message": "Hello from RipHTTPLib!",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "demo": true,
        "data": {
            "items": [1, 2, 3, 4, 5],
            "metadata": {
                "version": "1.0",
                "features": ["http1", "trailers", "chunked"]
            }
        }
    });

    let body = Bytes::from(request_body.to_string());

    // Print request details
    println!("\n📤 Outgoing Request:");
    println!("   Method: POST");
    println!("   Headers:");
    for header in &headers {
        if let Some(ref value) = header.value {
            println!("     {}: {}", header.name, value);
        } else {
            println!("     {}", header.name);
        }
    }
    println!("   Body Size: {} bytes", body.len());
    println!(
        "   Body Preview: {}...",
        String::from_utf8_lossy(&body[..std::cmp::min(100, body.len())]).replace('\n', "\\n")
    );

    println!("   Trailers (sent after body):");
    for trailer in &trailers {
        if let Some(ref value) = trailer.value {
            println!("     {}: {}", trailer.name, value);
        } else {
            println!("     {}", trailer.name);
        }
    }

    println!("\n🔄 Sending Request...");

    // Send POST request with trailers
    match client
        .post(&target, Some(headers), Some(body), Some(trailers))
        .await
    {
        Ok(response) => {
            println!("✅ Request Successful!");

            // Print response details
            println!("\n📥 Response Details:");
            println!("   Status: {}", response.status);
            println!("   Protocol: {}", response.protocol_version);

            println!("   Headers ({}):", response.headers.len());
            for header in &response.headers {
                if let Some(ref value) = header.value {
                    // Truncate long header values for readability
                    println!("     {}: {}", header.name, value.clone());
                } else {
                    println!("     {}", header.name);
                }
            }

            if let Some(ref trailers) = response.trailers {
                println!("   Response Trailers ({}):", trailers.len());
                for trailer in trailers {
                    if let Some(ref value) = trailer.value {
                        println!("     {}: {}", trailer.name, value);
                    } else {
                        println!("     {}", trailer.name);
                    }
                }
            } else {
                println!("   Response Trailers: None");
            }

            println!("   Body Size: {} bytes", response.body.len());

            // Try to parse and pretty-print JSON response
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                println!("   Body (JSON):");
                let pretty_json = serde_json::to_string_pretty(&json_value)?;
                // Print first few lines of the response
                for (i, line) in pretty_json.lines().enumerate() {
                    println!("     {}", line);
                }
            } else {
                // If not JSON, print raw body (truncated)
                let body_str = String::from_utf8_lossy(&response.body);
                let preview = if body_str.len() > 200 {
                    format!("{}", &body_str)
                } else {
                    body_str.to_string()
                };
                println!("   Body Preview: {}", preview.replace('\n', "\\n"));
            }

            // Demonstrate other HTTP methods
            println!("\n🔄 Testing other HTTP methods...");

            // GET request
            println!("\n📤 GET Request:");
            let get_target = parse_target("http://httpbin.org/get?demo=true&source=riphttplib");
            let get_headers = vec![Header::new(
                "Accept".to_string(),
                "application/json".to_string(),
            )];

            match client.get(&get_target, Some(get_headers), None, None).await {
                Ok(get_response) => {
                    println!(
                        "   ✅ GET Status: {} {}",
                        get_response.status, get_response.protocol_version
                    );
                    println!("   📊 Response size: {} bytes", get_response.body.len());
                }
                Err(e) => {
                    println!("   ❌ GET Failed: {}", e);
                }
            }

            // HEAD request
            println!("\n📤 HEAD Request:");
            match client
                .send(&get_target, Some("HEAD".to_string()), None, None, None)
                .await
            {
                Ok(head_response) => {
                    println!(
                        "   ✅ HEAD Status: {} {}",
                        head_response.status, head_response.protocol_version
                    );
                    println!(
                        "   📊 Body size (should be 0): {} bytes",
                        head_response.body.len()
                    );
                    println!("   📋 Headers count: {}", head_response.headers.len());
                }
                Err(e) => {
                    println!("   ❌ HEAD Failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ Request Failed: {}", e);
            println!("   Error Type: {:?}", e);

            // Provide helpful debugging information
            println!("\n🔍 Debugging Information:");
            println!("   - Check network connectivity");
            println!("   - Verify target host is reachable: {}", target.host);
            println!("   - Ensure port {} is accessible", target.port);
            if target.scheme == "https" {
                println!("   - Verify TLS/SSL configuration");
            }
        }
    }

    println!("\n🎯 Demo Features Demonstrated:");
    println!("   ✓ HTTP/1.1 POST request with JSON body");
    println!("   ✓ Custom headers");
    println!("   ✓ Trailers (HTTP chunked transfer encoding)");
    println!("   ✓ Protocol version detection");
    println!("   ✓ Response parsing (headers, body, trailers)");
    println!("   ✓ Multiple HTTP methods (GET, POST, HEAD)");
    println!("   ✓ Error handling and debugging info");

    println!("\n✨ Demo completed successfully!");

    Ok(())
}
