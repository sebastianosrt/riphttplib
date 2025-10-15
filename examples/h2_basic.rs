use riphttplib::h2::H2Client;
use riphttplib::types::{Header, Request};
use riphttplib::utils::parse_target;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let target = parse_target("https://httpbin.org/post")?;
    let target = parse_target("https://google.com/")?;
    let headers = vec![
        Header::new("accept".into(), "text/html".into()),
        Header::new("just".into(), "testing".into()),
    ];
    let body = "test";
    let trailers = vec![Header::new("trailer".into(), "test".into())];

    let request = Request::with_target(target, "POST")
        .with_headers(headers)
        .with_body(body)
        .with_trailers(Some(trailers));

    let client = H2Client::new();
    let response = client.send_request(request).await?;

    println!("\n{} {}", response.protocol_version, response.status);
    for header in response.headers {
        println!("{}", header.to_string());
    }
    println!("\n{}", String::from_utf8_lossy(&response.body));

    Ok(())
}
