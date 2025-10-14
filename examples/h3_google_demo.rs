use riphttplib::h3::H3Client;
use riphttplib::types::Request;
use riphttplib::utils::parse_target;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure ring provider is installed for rustls
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install ring provider");

    let client = H3Client::new();
    let target = parse_target("https://www.google.com/")?;

    println!("\n== HTTP/3 Request to {} ==", target.as_str());
    let request = Request::new("GET");
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
