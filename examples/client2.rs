use riphttplib::types::protocol::Client;
use riphttplib::H3;
use riphttplib::H2;
use riphttplib::H1;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let response = Client::<H2>::post("https://httpbin.org/post")
        .header("user-agent: aa")
        .headers(vec!["first: f".to_string(), "second: saa".to_string()])
        .data("dasdasdas")
        // .trailer("ssss: ssss")
        .await?;

    println!("\n{} {}", response.protocol, response.status);
    for header in &response.headers {
        println!("{}", header);
    }
    println!("{}", response.text());

    Ok(())
}
