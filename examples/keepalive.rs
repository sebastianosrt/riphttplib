use riphttplib::h1::H1;
use riphttplib::types::{ClientTimeouts, Request};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = H1::new();
    let req1 = Request::new("https://httpbin.org/get", "GET")?.query(vec![("req1","param")]).header("Connection: keep-alive");
    let req2 = Request::new("https://httpbin.org/get", "GET")?.query(vec![("req2","param")]);
    let timeouts = ClientTimeouts {
            connect: Some(Duration::from_secs(15)),
            read: Some(Duration::from_secs(45)),
            write: Some(Duration::from_secs(15)),
        };

    let mut connection = client.open_stream(&req1.clone(), &timeouts).await?;

    client.write_request(&mut connection, &req1, &timeouts).await?;
    let res = client.read_response(&mut connection, true, &timeouts).await?;
    println!("{}", res.text());

    client.write_request(&mut connection, &req2, &timeouts).await?;
    let res = client.read_response(&mut connection, true, &timeouts).await?;
    println!("{}", res.text());
    

    // let req = "GET / HTTP/1.1\r\nHost: quic.tech\r\n\r\n";
    // let res = client.send_raw("https://quic.tech", req.into()).await?;


    Ok(())
}
