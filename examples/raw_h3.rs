use riphttplib::connection::HttpConnection;
use riphttplib::h3::connection::{H3ConnectOptions, H3Connection, H3ReadOptions};
use riphttplib::types::{ClientTimeouts, FrameH3, FrameType, FrameTypeH3};
use riphttplib::Request;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://quic.tech:8443";
    send(url).await?;
    send_with_custom_handler(url).await?;
    Ok(())
}

async fn send(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let req = Request::new(url, "GET")?;
    let headers = Request::prepare_pseudo_headers(&req)?;

    // create connection
    let connect_options = H3ConnectOptions {
        target: url.to_string(),
        timeouts: ClientTimeouts::disabled(),
    };
    let mut connection =
        <H3Connection as HttpConnection>::connect(connect_options).await?;
    let (stream_id, mut send_stream) = connection.create_request_stream().await?;

    // create headers frame
    let header_block = connection.encode_headers(stream_id, &headers).await?;
    let headers_frame = FrameH3::header(stream_id, header_block);
    let serialized_headers = headers_frame.serialize()?;

    // write frame
    send_stream.write_all(&serialized_headers).await?;
    send_stream.finish()?;

    // read response
    let response = <H3Connection as HttpConnection>::read_response(
        &mut connection,
        H3ReadOptions {
            stream_id,
            timeouts: None,
        },
    )
    .await?;

    println!("{} {}", response.protocol, response.status);
    for header in &response.headers {
        println!("{}", header);
    }
    if !response.body.is_empty() {
        println!("\n{}", String::from_utf8_lossy(&response.body));
    }

    println!();
    Ok(())
}

async fn send_with_custom_handler(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let req = Request::new(url, "GET")?;
    let headers = Request::prepare_pseudo_headers(&req)?;

    let timeouts = ClientTimeouts::disabled();
    let connect_options = H3ConnectOptions {
        target: url.to_string(),
        timeouts: timeouts.clone(),
    };
    let mut connection =
        <H3Connection as HttpConnection>::connect(connect_options).await?;
    let (stream_id, mut send_stream) = connection.create_request_stream().await?;

    let header_block = connection.encode_headers(stream_id, &headers).await?;
    let headers_frame = FrameH3::header(stream_id, header_block);
    let serialized_headers = headers_frame.serialize()?;
    send_stream.write_all(&serialized_headers).await?;
    send_stream.finish()?;

    // custom event handler
    let event_handler = |frame: &FrameH3| match &frame.frame_type {
        FrameType::H3(FrameTypeH3::Headers) => {
            println!(
                "[stream {}] HEADERS ({} bytes)",
                frame.stream_id,
                frame.payload.len()
            );
        }
        FrameType::H3(FrameTypeH3::Data) => {
            println!("[stream {}] DATA", frame.stream_id);
            println!("  {}", String::from_utf8_lossy(&frame.payload));
        }
        FrameType::H3(other) => {
            println!("[stream {}] {:?}", frame.stream_id, other);
        }
        _ => {}
    };

    let response = connection
        .read_response_with_timeouts(stream_id, &timeouts, Some(&event_handler))
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
