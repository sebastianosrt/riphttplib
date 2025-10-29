use riphttplib::h1::H1Client;
use riphttplib::h2::H2Client;
use riphttplib::h3::H3Client;
use riphttplib::types::{ClientTimeouts, Header, Protocol, Request};
use std::time::Duration;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headers = vec![Header::new("accept".into(), "text/html".into()), Header::new("trailers".into(), "trailer".into()), Header::new("TE".into(), "trailers".into())];
    let body = "test";
    let trailers = vec![Header::new("trailer".into(), "test".into())];

    let request = Request::new("https://quic.tech:8443", "GET")?
        .header(Header::new("User-Agent".to_string(), "riphttplib/0.1.0".to_string()))
        .headers(headers)
        .body(body)
        .json(json!({ "test": "value" }))
        .params(vec![("test", "h3-features")])
        .cookies(vec![("session", "test")])
        .timeout(ClientTimeouts {
            connect: Some(Duration::from_secs(15)),
            read: Some(Duration::from_secs(45)),
            write: Some(Duration::from_secs(15)),
        })
        .allow_redirects(true)
        .trailers(Some(trailers));

    {
        let client = H1Client::new();
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
    {        
        let client = H2Client::new();
        let response = client.response(request.clone()).await?;
     
        println!("\nHTTP/2");
    
        println!("\n{} {}", response.protocol, response.status);
        for header in &response.headers {
            println!("{}", header);
        }
        println!("\n{}", response.text());
        if let Some(frames) = &response.frames {
            println!("Captured {} frame(s)", frames.len());
        }
    }
    {
        println!("\nHTTP/3");

        let client = H3Client::new();
        let response = client.response(request.clone()).await?;
    
        println!("\n{} {}", response.protocol, response.status);
        for header in &response.headers {
            println!("{}", header);
        }
        println!("\n{}", response.text());
        if let Some(frames) = &response.frames {
            println!("Captured {} frame(s)", frames.len());
        }
    }

    Ok(())
}
