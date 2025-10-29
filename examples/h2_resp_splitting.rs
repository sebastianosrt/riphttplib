use riphttplib::h2::H2Client;
use riphttplib::types::{Header, Protocol, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headers = vec![Header::new("accept".into(), "text/html\r\naaa:ss".into())];
    let body = "test";
    let trailers = vec![Header::new(
        "trailer\r\ninjected: header".into(),
        "test".into(),
    )];

    let request = Request::new("https://localhost:24443", "GET")?
        .header(Header::new(
            "User-Agent".to_string(),
            "riphttplib/0.1.0".to_string(),
        ))
        .headers(headers)
        .body(body)
        .trailers(Some(trailers));

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

    Ok(())
}
