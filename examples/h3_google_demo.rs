use riphttplib::h3::H3Client;
use riphttplib::types::{Header, Request};
use riphttplib::utils::parse_target;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure ring provider is installed for rustls
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install ring provider");

    let client = H3Client::new();
    // let target = parse_target("https://127.0.0.1:31443")?;
    // let target = parse_target("https://127.0.0.1:28443")?;
    // let target = parse_target("https://127.0.0.1:15444")?;
    // let target = parse_target("https://127.0.0.1:11444")?; // haproxy

    let target = parse_target("https://www.google.com")?;

    println!("\n== HTTP/3 Request to {} ==", target.as_str());
    let request = Request::new("GET")
        .with_headers(vec![
            Header::new("Content-Type".to_string(), "application/json".to_string()),
            Header::new("X-Custom-Header".to_string(), "demo-value".to_string()),
            Header::new("Accept".to_string(), "*/*".to_string()),
            Header::new("User-Agent".to_string(), "RipHTTPLib-Demo/1.0".to_string()),
        ])
        .with_trailers(Some(vec![
            Header::new("trailxxxxxxx".to_string(), "xxxxxxxxxxRipHTTPLib-Demo/1.0".to_string()),
            Header::new("aapath".to_string(), "/xx\r\nxxxxxxxxRipHTTPLib-Demo/1.0".to_string()),
        ]));
    match client.send_request(&target, request).await {
        Ok(response) => {
            println!("Status: {}", response.status);
            println!("\n-- Response Headers --");
            for header in &response.headers {
                if let Some(ref value) = header.value {
                    println!("{}: {}", header.name, value);
                } else {
                    println!("{}", header.name);
                }
            }

            if let Some(ref trailers) = response.trailers {
                println!("\n-- Trailers --");
                for trailer in trailers {
                    if let Some(ref value) = trailer.value {
                        println!("{}: {}", trailer.name, value);
                    } else {
                        println!("{}", trailer.name);
                    }
                }
            }

            println!("\n-- Body --");
            let body_preview = String::from_utf8_lossy(&response.body);
            println!("{}", body_preview);
        }
        Err(err) => {
            eprintln!("HTTP/3 request failed: {}", err);
        }
    }

    Ok(())
}
