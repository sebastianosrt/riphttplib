use riphttplib::h1::H1;
use riphttplib::h2::H2;
use riphttplib::h3::H3;
use riphttplib::types::{ClientTimeouts, Header, Protocol, Request};
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headers = vec![
        Header::new("accept".into(), "text/html".into()),
        Header::new("trailers".into(), "trailer".into()),
        Header::new("TE".into(), "trailers".into()),
    ];
    let body = "test";
    let trailers = vec![Header::new("trailer".into(), "test".into())];

    let request = Request::new("https://quic.tech:8443", "GET")?
        .header(Header::new(
            "User-Agent".to_string(),
            "riphttplib/0.1.0".to_string(),
        ))
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
        .follow_redirects(true)
        .trailers(trailers);

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
    {
        let client = H2::new();
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

        let client = H3::new();
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
