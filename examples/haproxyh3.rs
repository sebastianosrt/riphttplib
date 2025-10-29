use riphttplib::h3::H3Client;
use riphttplib::types::{Header, Protocol, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headers = vec![
        Header::new("trailers".into(), "trailer".into()),
        Header::new("accept".into(), "text/htmln\na:a".into()),
        Header::new_valueless("valueless".into()),
    ];
    let body = "test";
    let trailers = vec![
        Header::new("trailer".into(), "testa".into()),
        Header::new_valueless("sagetdasd\r\na".into()),
    ];

    let request = Request::new("https://localhost:32444", "GET")?
        .header(Header::new(
            "User-Agent".to_string(),
            "riphttplib/0.1.0".to_string(),
        ))
        .headers(headers)
        .body(body)
        .trailers(Some(trailers));

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

    Ok(())
}
