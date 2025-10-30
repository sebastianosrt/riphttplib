use riphttplib::h1::H1;
use riphttplib::h2::H2;
use riphttplib::h3::H3;
use riphttplib::types::{ClientTimeouts, Header, Protocol, Request};
use serde_json::json;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let timeouts = ClientTimeouts {
        connect: Some(CONNECT_TIMEOUT),
        read: Some(IO_TIMEOUT),
        write: Some(IO_TIMEOUT),
    };

    let request = Request::new("https://mail1.www.cozykitchencovers.etsy.com", "POST")?
            .header(Header::new(
                "user-agent".to_string(),
                "Mozilla/5.0 (X11; Linux x86_64; rv:142.0) Gecko/20100101 Firefox/142.0".to_string(),
            ))
            .header(Header::new("bug-bounty".to_string(), "scan".to_string()))
            .header(Header::new("te".to_string(), "trailers".to_string()))
            .body("aaaaaaaaa")
            .trailer(Header::new("test".to_string(), "test".to_string()))
            .trailer(Header::new("content-length".to_string(),"100000".to_string(),))
            // .trailer(Header::new("expect".to_string(), "100-continue".to_string()))
            .timeout(timeouts.clone())
            .follow_redirects(false);
    {
        
        // let client = H3::new();
        // let response = client.response(request.clone()).await?;
        
        let response = H3::timeouts(timeouts.clone()).send_request(request).await?;

        println!("\n{} {}", response.protocol, response.status);
        for header in &response.headers {
            println!("{}", header);
        }
        // println!("\n{}", response.text());
        // println!("\n{}", String::from_utf8_lossy(&response.body));
        // if let Some(frames) = &response.frames {
        //     println!("Captured {} frame(s)", frames.len());
        // }
    }

    Ok(())
}
