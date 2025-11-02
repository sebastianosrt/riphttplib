use riphttplib::h3::connection::H3Connection;
use riphttplib::types::{FrameH3, FrameType, FrameTypeH3};
use riphttplib::Request;
use tokio::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://quic.tech:8443";

    let req = Request::new(url, "GET")?;
    let headers = Request::prepare_pseudo_headers(&req)?;

    let mut connection = H3Connection::connect(url).await?;
    let (stream_id, mut send_stream) = connection.create_request_stream().await?;

    let header_block = connection.encode_headers(stream_id, &headers).await?;
    let headers_frame = FrameH3::header(stream_id, header_block);
    let serialized_headers = headers_frame.serialize()?;
    send_stream.write_all(&serialized_headers).await?;
    send_stream.finish()?;

    let overall_timeout = Duration::from_secs(10);
    let per_event_timeout = Duration::from_millis(500);
    let max_events = 20;
    let start = Instant::now();

    let mut event_count = 0usize;
    let mut status: Option<u16> = None;

    while start.elapsed() < overall_timeout && event_count < max_events {
        if let Ok(result) = tokio::time::timeout(per_event_timeout, connection.poll_control()).await
        {
            result?;
        }

        let frame_result =
            tokio::time::timeout(per_event_timeout, connection.read_request_frame(stream_id)).await;

        let frame_opt = match frame_result {
            Ok(inner) => inner?,
            Err(_) => continue,
        };

        let frame = match frame_opt {
            Some(frame) => frame,
            None => {
                connection.stream_finished_receiving(stream_id)?;
                let _ = connection.remove_closed_stream(stream_id);
                println!("Stream closed by server");
                break;
            }
        };

        event_count += 1;

        match &frame.frame_type {
            FrameType::H3(FrameTypeH3::Headers) => {
                let decoded = connection.decode_headers(stream_id, &frame.payload).await?;
                println!("HEADERS");
                for header in &decoded {
                    println!("  {}", header);
                }

                if status.is_none() {
                    if let Some(code) = decoded.iter().find_map(|h| {
                        (h.name == ":status")
                            .then(|| h.value.as_ref()?.parse::<u16>().ok())
                            .flatten()
                    }) {
                        status = Some(code);
                        println!("  (status: {})", code);
                    }
                }
            }
            FrameType::H3(FrameTypeH3::Data) => {
                println!("DATA");
                println!("  {}", String::from_utf8_lossy(&frame.payload));
            }
            FrameType::H3(other) => {
                println!("FRAME {:?}", other);
            }
            _ => {}
        }

        connection.handle_frame(&frame).await?;
    }

    if status.is_none() {
        println!("No response received before timeout");
    }

    Ok(())
}
