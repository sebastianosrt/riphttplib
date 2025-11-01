use riphttplib::h1::H1;
use riphttplib::h2::H2;
use riphttplib::h3::H3;
use riphttplib::types::{ClientTimeouts, Protocol, Request};
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headers = vec![
        "accept: text/html".to_string(),
        "trailers: trailer".to_string(),
        "TE: trailers".to_string(),
    ];
    let body = "test";
    let trailers = vec!["trailer: test".to_string()];

    // let request = Request::new("https://httpbin.org/get", "GET")?
    let request = Request::new("https://httpbin.org/get", "GET")?;
        // .body(body)
        // .json(json!({ "test": "value" }));
        // .params(vec![("test", "h3-features")])
        // .cookies(vec![("session", "test")])
        // .timeout(ClientTimeouts {
        //     connect: Some(Duration::from_secs(15)),
        //     read: Some(Duration::from_secs(45)),
        //     write: Some(Duration::from_secs(15)),
        // })
        // .follow_redirects(true)
        // .trailers(trailers);
    
    for header in &request.headers {
        println!("{}", header);
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

    Ok(())
}
