use riphttplib::h1::H1;
use riphttplib::h2::H2;
use riphttplib::h3::H3;
use riphttplib::types::{ClientTimeouts, Header, Protocol, Request};
use serde_json::json;
use std::time::Duration;
use riphttplib::types::protocol::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let req = "GET / HTTP/1.1\r\nHost: quic.tech\r\n\r\n";
    let client = H1::new();
    let res = client.send_raw("https://quic.tech", req.into()).await?;

    println!("{}", res.status);

    Ok(())
}
