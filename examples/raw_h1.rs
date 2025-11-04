use riphttplib::connection::HttpConnection;
use riphttplib::h1::connection::{H1ConnectOptions, H1Connection, H1ReadOptions};
use riphttplib::types::ClientTimeouts;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let req = "GET / HTTP/1.1\r\nHost: quic.tech\r\n\r\n";
    let connect_options = H1ConnectOptions {
        target: "https://quic.tech".to_string(),
        timeouts: ClientTimeouts::default(),
    };
    let mut connection =
        <H1Connection as HttpConnection>::connect(connect_options).await?;

    let client_clone = connection.client().clone();
    let write_timeout = client_clone.get_timeouts().write;
    {
        let stream = connection.stream_mut();
        client_clone
            .write_to_stream(stream, req.as_bytes(), write_timeout)
            .await?;
    }

    let response = <H1Connection as HttpConnection>::read_response(
        &mut connection,
        H1ReadOptions { read_body: true },
    )
    .await?;

    println!("{}", response.status);

    Ok(())
}
