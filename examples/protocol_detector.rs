use riphttplib::types::protocol::HttpProtocol;
use riphttplib::utils::detect_protocol;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://quic.tech";
    println!("Detecting supported protocols for {}", url);

    match detect_protocol(url).await {
        Ok(result) => {
            println!("Target: {}", result.target);
            if result.supported.is_empty() {
                println!("No protocols detected");
            } else {
                println!("Supported protocols:");
                for protocol in &result.supported {
                    match protocol {
                        HttpProtocol::Http1 => println!(" - HTTP/1.1"),
                        HttpProtocol::Http2 => println!(" - HTTP/2"),
                        HttpProtocol::H2C => println!(" - HTTP/2 (h2c)"),
                        HttpProtocol::Http3 => println!(" - HTTP/3"),
                    }
                }
            }
        }
        Err(err) => {
            eprintln!("Failed to detect protocols: {}", err);
        }
    }

    Ok(())
}
