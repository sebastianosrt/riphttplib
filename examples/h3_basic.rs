use riphttplib::h3::H3Client;
use riphttplib::types::{Header, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headers = vec![
        Header::new("accept".into(), "text/html".into()),
        Header::new("just".into(), "testing".into()),
    ];
    let body = "test";
    let trailers = vec![Header::new("trailer".into(), "test".into())];

    let request = Request::new("https://www.google.com", "POST")?
        .with_headers(headers)
        .with_body(body)
        .with_trailers(Some(trailers));

    let client = H3Client::new();
    let response = client.send_request(request).await?;

    println!("\n{} {}", response.protocol_version, response.status);
    for header in response.headers {
        println!("{}", header.to_string());
    }
    println!("\n{}", String::from_utf8_lossy(&response.body));

    Ok(())
}
