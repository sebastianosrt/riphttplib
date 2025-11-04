use riphttplib::h1::H1;
use riphttplib::h2::H2;
use riphttplib::h3::H3;
use riphttplib::types::{ClientTimeouts, Protocol, Request};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headers = vec![
        "accept: text/html".to_string(),
        "trailers: trailer".to_string(),
        "TE: trailers".to_string(),
    ];
    let trailers = vec!["trailer: test".to_string()];

    // create request
    let request = Request::new("https://quic.tech:8443", "GET")?
        .headers(headers)
        .query(vec![("test", "param")])
        .cookies(vec![("session", "test")])
        .body("body")
        .trailers(trailers)
        .timeout(ClientTimeouts {
            connect: Some(Duration::from_secs(15)),
            read: Some(Duration::from_secs(45)),
            write: Some(Duration::from_secs(15)),
        })
        .follow_redirects(true);

    // send request with each protocol
    {
        let client = H1::new();
        let response = client.response(request.clone()).await?;

        println!("HTTP/1.1");

        println!("{}", response);    
    }
    {
        let client = H2::new();
        let response = client.response(request.clone()).await?;

        println!("\nHTTP/2");

        println!("{}", response);
        if let Some(frames) = &response.frames {
            println!("Captured {} frame(s)", frames.len());
        }
    }
    {
        println!("\nHTTP/3");

        let client = H3::new();
        let response = client.response(request.clone()).await?;

        println!("{}", response);
        if let Some(frames) = &response.frames {
            println!("Captured {} frame(s)", frames.len());
        }
    }

    Ok(())
}
