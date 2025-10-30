use riphttplib::h1::H1Client;
use riphttplib::h2::H2Client;
use riphttplib::h3::H3Client;
use riphttplib::types::{ClientTimeouts, Header, Protocol, Request};
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let response = H2Client::post("https://httpbin.org/post")
        .header("aaa: aa")
        .headers(["first: f", "second: saa"])
        .body("dasdasdas")
        .trailer("ssss: ssss")
        .await?;

    // println!("\nHTTP/2");

    println!("\n{} {}", response.protocol, response.status);
    for header in &response.headers {
        println!("{}", header);
    }
    println!("{}", response.text());

    Ok(())
}
