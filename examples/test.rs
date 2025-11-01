use riphttplib::h1::H1;
use riphttplib::h2::H2;
use riphttplib::h3::H3;
use riphttplib::types::{ClientTimeouts, Protocol, Request};
use serde_json::json;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let timeouts = ClientTimeouts {
        connect: Some(CONNECT_TIMEOUT),
        read: Some(IO_TIMEOUT),
        write: Some(IO_TIMEOUT),
    };

    let request = Request::new("https://turnip.prd.mangosteen.sec.xvservice.net", "POST")?
            .header("user-agent: Mozilla/5.0 (X11; Linux x86_64; rv:142.0) Gecko/20100101 Firefox/142.0")
            .header("bug-bounty: scan")
            .header("te: trailers")
            .body("aaaaaaa")
            .trailer("test: test")
            .trailer("test: testaaaaaaaaaaaaaaaa")
            .trailer("content-length: 10000")
            .trailer("expect: 100-continue")
            .timeout(timeouts.clone())
            .follow_redirects(false);
    {
        
        // let client = H3::new();
        // let response = client.response(request.clone()).await?;
        
        let response = H1::timeouts(timeouts.clone()).send_request(request).await?;

        println!("\n{} {}", response.protocol, response.status);
        for header in &response.headers {
            println!("{}", header);
        }
        match response.trailers {
            Some(val) => {
                println!("\ntrailers\n");
                for header in &val {
                    println!("{}", header);
                }
            },
            None => {}
        }
        // println!("\n{}", response.text());
        // println!("\n{}", String::from_utf8_lossy(&response.body));
        // if let Some(frames) = &response.frames {
        //     println!("Captured {} frame(s)", frames.len());
        // }
    }

    Ok(())
}
