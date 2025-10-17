use bytes::Bytes;
use riphttplib::{prepare_pseudo_headers, FrameBuilderExt, FrameTypeH2, Request};
use riphttplib::types::{Header, ClientTimeouts, FrameH2};
use riphttplib::h2::connection::{H2Connection, StreamEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://localhost:8843";
    let timeout = ClientTimeouts::disabled();

    let req = Request::new(url, "GET")?;
    let headers = prepare_pseudo_headers(&req)?;
    let mut connection = H2Connection::connect(url, &timeout).await?;
    let mut stream_id = connection.create_stream().await?;

    for _i in 1..20000 {
        stream_id = connection.create_stream().await?;
        print!("{}\n", stream_id);
         let mut payload = Vec::new();
        payload.extend_from_slice(&stream_id.to_be_bytes()); // Stream dependency = self
        payload.push(0); // Weight

        // FrameH2::header(stream_id, &headers, false, true)?
        FrameH2::header(stream_id, &headers, true, true)?
            // .chain(FrameH2::data(stream_id, "any".into(), false))
            .chain(FrameH2::header(stream_id, &headers, false, true)?)
            // .chain(FrameH2::new(
            //     FrameTypeH2::WindowUpdate,
            //     0,
            //     stream_id,
            //     bytes::Bytes::from(0u32.to_be_bytes().to_vec())
            // ))
            // .chain(FrameH2::new(
            //     FrameTypeH2::Priority,
            //     0,
            //     stream_id,
            //     Bytes::from(payload)
            // ))
            // .chain(FrameH2::new(
            //     FrameTypeH2::Priority,
            //     0,
            //     stream_id,
            //     Bytes::from(vec![0x00, 0x00, 0x01])
            // ))
            // .chain(FrameH2::new(
            //     FrameTypeH2::WindowUpdate,
            //     0,
            //     stream_id,
            //     Bytes::from(u32::MAX.to_be_bytes().to_vec())
            // ))
            .send(&mut connection).await?;
        print!("{}\n",connection.streams[&stream_id].state);
        
    }

    let response_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count_clone = response_count.clone();

    let event_handler = move |event: &StreamEvent| {
        let count = count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        println!("Response {}: {:?}", count, event);

        match event {
            StreamEvent::Headers { headers, end_stream, is_trailer } => {
                println!("  → HEADERS response (end_stream: {}, is_trailer: {})", end_stream, is_trailer);
                for header in headers {
                    println!("    {}: {:?}", header.name, header.value);
                }
                if *end_stream {
                    println!("  → Stream ended with headers");
                }
            }
            StreamEvent::Data { payload, end_stream } => {
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

    match connection.read_response_options(
        stream_id,
        Some(std::time::Duration::from_secs(5)), // overall timeout
        Some(std::time::Duration::from_millis(1000)), // event timeout
        Some(100000), // max events
        Some(&event_handler)
    ).await {
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