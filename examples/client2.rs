use riphttplib::types::protocol::Client;
use riphttplib::H3Client;
use riphttplib::H2Client;
use riphttplib::H1Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let response = Client::<H2Client>::post("https://httpbin.org/post")
        .header("user-agent: aa")
        .headers(["first: f", "second: saa"])
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
