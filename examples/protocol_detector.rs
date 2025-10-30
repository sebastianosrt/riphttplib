use riphttplib::types::protocol::HttpProtocol;
use riphttplib::detector::{detect_protocol};
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let program_start = Instant::now();

    let url = "https://quic.tech";
    println!("Detecting supported protocols for {}", url);

    let detect_start = Instant::now();
    let result = detect_protocol(url).await;
    let detect_elapsed = detect_start.elapsed();

    match result {
        Ok(result) => {
            if result.is_empty() {
                println!("No protocols detected");
            } else {
                println!("Supported protocols:");
                for detected in &result {
                    match detected.protocol {
                        HttpProtocol::Http1 => println!(" - HTTP/1.1"),
                        HttpProtocol::Http2 => println!(" - HTTP/2"),
                        HttpProtocol::H2C => println!(" - HTTP/2 (h2c)"),
                        HttpProtocol::Http3 => println!(" - HTTP/3"),
                    }
                }
            }
            println!("Detection completed in {:?}", detect_elapsed);
        }
        Err(err) => {
            eprintln!("Failed to detect protocols: {}", err);
            println!("Detection failed after {:?}", detect_elapsed);
        }
    }

    println!("Total runtime: {:?}", program_start.elapsed());
    Ok(())
}
