use riphttplib::types::protocol::Client;
use riphttplib::H2;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let response = Client::<H2>::post("https://httpbin.org/post")
        .header("user-agent: aa")
        .query(vec![("test", "param")])
        .headers(vec![
            "test: header".to_string(),
            "second: header".to_string(),
        ])
        .data(vec![("test", "param")])
        // .json(json!({"test": "json"}))
        .await?;

    println!("\n{} {}", response.protocol, response.status);
    for header in &response.headers {
        println!("{}", header);
    }
    println!("{}", response.text());

    Ok(())
}
