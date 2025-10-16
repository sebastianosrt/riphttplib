use riphttplib::h1::H1Client;
use riphttplib::h2::H2Client;
use riphttplib::h3::H3Client;
use riphttplib::types::{Header, Request, ClientTimeouts};
use std::time::Duration;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headers = vec![Header::new("accept".into(), "text/html".into())];
    let body = "test";
    let trailers = vec![Header::new("trailer".into(), "test".into())];

    let request = Request::new("https://quic.tech:8443", "GET")?
        .header(Header::new("User-Agent".to_string(), "riphttplib/0.1.0".to_string()))
        .headers(headers)
        .body(body)
        .json(json!({ "test": "value" }))
        .params(vec![("test", "h3-features")])
        .cookies(vec![("session", "test")])
        .timeout(ClientTimeouts {
            connect: Some(Duration::from_secs(15)),
            read: Some(Duration::from_secs(45)),
            write: Some(Duration::from_secs(15)),
        })
        .allow_redirects(true)
        .trailers(Some(trailers));

    {
        let client = H1Client::new();
        let response = client.send_request(request.clone()).await?;

        println!("HTTP/1.1");
    
        println!("\n{} {}", response.protocol, response.status);
        for header in &response.headers {
            println!("{}", header);
        }
        // println!("\n{}", response.text());
        println!("\n{}", String::from_utf8_lossy(&response.body));
    }
    {        
        let client = H2Client::new();
        let response = client.send_request(request.clone()).await?;
     
        println!("\nHTTP/2");
    
        println!("\n{} {}", response.protocol, response.status);
        for header in &response.headers {
            println!("{}", header);
        }
        println!("\n{}", response.text());
    }
    {
        println!("\nHTTP/3");

        let client = H3Client::new();
        let response = client.send_request(request.clone()).await?;
    
        println!("\n{} {}", response.protocol, response.status);
        for header in &response.headers {
            println!("{}", header);
        }
        println!("\n{}", response.text());
    }

    // match client.send_request(request).await {
    //     Ok(response) => {
    //         println!("✓ Status: {}", response.status);
    //         println!("✓ Protocol: {}", response.protocol);
    //         println!("✓ Response length: {} bytes", response.body.len());

    //         // Show a preview of the response
    //         let response_text = String::from_utf8_lossy(&response.body);
    //         if response_text.len() > 300 {
    //             println!("✓ Response preview: {}...", &response_text[..300]);
    //         } else {
    //             println!("✓ Response: {}", response_text);
    //         }
    //     }
    //     Err(e) => {
    //         eprintln!("✗ Request failed: {}", e);
    //     }
    // }

    Ok(())
}
