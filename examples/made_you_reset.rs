use bytes::Bytes;
use riphttplib::h2::connection::H2Connection;
use riphttplib::types::{ClientTimeouts, FrameH2};
use riphttplib::{FrameBuilderExt, FrameTypeH2, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://localhost:8443";
    let timeout = ClientTimeouts::disabled();

    let req = Request::new(url, "GET")?;
    let headers = Request::prepare_pseudo_headers(&req)?;
    let mut connection = H2Connection::connect(url, &timeout).await?;

    for _i in 1..2000000000 {
        let stream_id = connection.create_stream().await?;
        print!("{}\n", stream_id);
        let mut payload = Vec::new();
        payload.extend_from_slice(&stream_id.to_be_bytes()); // Stream dependency = self
        payload.push(0); // Weight

        FrameH2::header(stream_id, &headers, false, true)?
            // FrameH2::header(stream_id, &headers, true, true)?
            // .chain(FrameH2::data(stream_id, "any".into(), false))
            // .chain(FrameH2::header(stream_id, &headers, false, true)?)
            .chain(FrameH2::new(
                FrameTypeH2::WindowUpdate,
                0,
                stream_id,
                Bytes::from(0u32.to_be_bytes().to_vec()),
            ))
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
            .send(&mut connection)
            .await?;
        print!("{}\n", connection.streams[&stream_id].state);
    }

    Ok(())
}
