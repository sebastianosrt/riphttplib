use riphttplib::h2::connection::H2Connection;
use riphttplib::types::{ClientTimeouts, FrameH2, Header};
use riphttplib::{prepare_pseudo_headers, FrameBuilderExt, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let url = "https://localhost:8843";
    let url = "http://localhost:7777";
    let timeout = ClientTimeouts::disabled();

    let req = Request::new(url, "GET")?;
    let headers = prepare_pseudo_headers(&req)?;
    let cont_header = vec![Header::new("x-test".into(), "continuation-data".into())];

    for _i in 1..1000 {
        let mut connection = H2Connection::connect(url, &timeout).await?;
        let stream_id = connection.create_stream().await?;

        FrameH2::header(stream_id, &headers, false, false)?
            .chain(FrameH2::continuation(stream_id, &cont_header, false)?.repeat(10))
            .chain(FrameH2::continuation(stream_id, &cont_header, true)?)
            .send(&mut connection)
            .await?;
    }

    Ok(())
}
