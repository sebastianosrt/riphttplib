use riphttplib::h2::connection::H2Connection;
use riphttplib::types::{ClientTimeouts, FrameBatch, Header, Request};
use riphttplib::utils::{ensure_user_agent, merge_headers, parse_target, prepare_pseudo_headers};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = parse_target("https://httpbin.org/get")?;

    let mut connection = H2Connection::connect(&target, &ClientTimeouts::default()).await?;
    let stream_id = connection.create_stream().await?;

    let request = Request::with_target(target.clone(), "GET")
        .with_header(Header::new("accept".into(), "text/plain".into()));
    let mut headers = merge_headers(prepare_pseudo_headers(&request, &target)?, &request);
    ensure_user_agent(&mut headers);

    // Build HEADERS/CONTINUATION frames with the convenience helpers.
    let frames = connection.build_headers_frames(stream_id, &headers, true)?;
    FrameBatch::new(frames).send_all(&mut connection).await?;
    connection.flush().await?;

    let mut body = Vec::new();
    loop {
        match connection.recv_stream_event(stream_id).await? {
            riphttplib::h2::connection::StreamEvent::Headers { end_stream, .. } if end_stream => {
                break;
            }
            riphttplib::h2::connection::StreamEvent::Headers { .. } => {}
            riphttplib::h2::connection::StreamEvent::Data {
                payload,
                end_stream,
            } => {
                body.extend_from_slice(&payload);
                if end_stream {
                    break;
                }
            }
            riphttplib::h2::connection::StreamEvent::RstStream { error_code } => {
                eprintln!("stream reset: {:?}", error_code);
                break;
            }
        }
    }

    println!("{}", String::from_utf8_lossy(&body));
    Ok(())
}
