use riphttplib::connection::HttpConnection;
use riphttplib::h2::connection::{H2ConnectOptions, H2Connection, StreamEvent};
use riphttplib::types::{ClientTimeouts, FrameH2};
use riphttplib::Request;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://quic.tech";
    send(url).await?;
    send_with_event_handler(url).await?;
    Ok(())
}

async fn send(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = ClientTimeouts::disabled();

    // prepare request headers
    let req = Request::new(url, "GET")?;
    let headers = Request::prepare_pseudo_headers(&req)?;

    // connect
    let connect_options = H2ConnectOptions {
        target: url.to_string(),
        timeouts: timeout.clone(),
    };
    let mut connection =
        <H2Connection as HttpConnection>::connect(connect_options).await?;
    let stream_id = connection.create_stream().await?;

    // write frame to connection
    FrameH2::header(stream_id, &headers, true, true)?
        .send(&mut connection)
        .await?;

    // read response
    let response =
        <H2Connection as HttpConnection>::read_response(&mut connection, stream_id).await?;

    println!("{}", response.text());
    println!();
    Ok(())
}

async fn send_with_event_handler(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = ClientTimeouts::disabled();

    let req = Request::new(url, "GET")?;
    let headers = Request::prepare_pseudo_headers(&req)?;

    let connect_options = H2ConnectOptions {
        target: url.to_string(),
        timeouts: timeout.clone(),
    };
    let mut connection =
        <H2Connection as HttpConnection>::connect(connect_options).await?;
    let stream_id = connection.create_stream().await?;

    FrameH2::header(stream_id, &headers, true, true)?
        .send(&mut connection)
        .await?;

    // custom event handler
    let handler = |event: &StreamEvent| match event {
        StreamEvent::Headers {
            headers,
            end_stream,
            is_trailer,
        } => {
            let kind = if *is_trailer { "TRAILERS" } else { "HEADERS" };
            println!(
                "[stream {stream_id}] {kind} (end_stream={end_stream})",
                kind = kind,
                end_stream = end_stream
            );
            for header in headers {
                println!("  {}", header);
            }
        }
        StreamEvent::Data { payload, end_stream } => {
            println!(
                "[stream {stream_id}] DATA (end_stream={})",
                end_stream
            );
            println!("  {}", String::from_utf8_lossy(payload));
        }
        StreamEvent::RstStream { error_code } => {
            println!("[stream {stream_id}] RST_STREAM {:?}", error_code);
        }
    };

    let response = connection
        .read_response_options(
            stream_id,
            Some(Duration::from_secs(10)),
            Some(Duration::from_millis(500)),
            None,
            Some(&handler),
        )
        .await?;

    println!("{} {}", response.protocol, response.status);
    for header in &response.headers {
        println!("{}", header);
    }
    if !response.body.is_empty() {
        println!("\n{}", String::from_utf8_lossy(&response.body));
    }

    Ok(())
}
