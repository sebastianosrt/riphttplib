use riphttplib::h1::H1;
use riphttplib::h2::H2;
use riphttplib::h3::H3;
use riphttplib::types::{ClientTimeouts, Protocol, Request};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let _proxy_settings = ProxySettings::parse(
    //     Some("http://localhost:8080"),
    //     Some("http://localhost:8080"),
    //     Some("socks5://127.0.0.1:9050"),
    // )?;

    let client = H1::new();
    let request = Request::new("http://httpbin.org/ip", "GET")?.proxy("http://localhost:8081")?;

    // http proxy
    let response = client.response(request.clone()).await?;
    println!("\n{}", response.text());

    // socks5
    let request = request.proxy("socks5://127.0.0.1:9050")?;
    let response = client.response(request.clone()).await?;
    println!("\n{}", response.text());

    Ok(())
}
