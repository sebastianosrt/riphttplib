use async_trait::async_trait;
use bytes::Bytes;
use riphttplib::types::{
    FrameBatch, FrameBuilderExt, FrameH2, FrameH3, FrameSink, FrameType, FrameTypeH2, FrameTypeH3,
    ProtocolError,
};

struct MockSink<F> {
    frames: Vec<F>,
}

#[async_trait(?Send)]
impl<F> FrameSink<F> for MockSink<F> {
    async fn write_frame(&mut self, frame: F) -> Result<(), ProtocolError> {
        self.frames.push(frame);
        Ok(())
    }
}

impl<F> MockSink<F> {
    fn new() -> Self {
        Self { frames: Vec::new() }
    }

    fn frames(&self) -> &[F] {
        &self.frames
    }
}

#[tokio::test]
async fn repeat_and_send_h2_frames() {
    let frame = FrameH2::new(FrameTypeH2::Data, 0, 1, Bytes::from_static(b"payload"));
    let mut sink = MockSink::<FrameH2>::new();

    frame
        .clone()
        .repeat(3)
        .send_all(&mut sink)
        .await
        .expect("send repeat frames");

    assert_eq!(sink.frames().len(), 3);
    assert_eq!(sink.frames()[0].payload, Bytes::from_static(b"payload"));
}

#[tokio::test]
async fn chain_h2_frames() {
    let frame_a = FrameH2::new(FrameTypeH2::Ping, 0, 0, Bytes::from_static(b"ping"));
    let frame_b = FrameH2::new(FrameTypeH2::Ping, 0, 0, Bytes::from_static(b"pong"));
    let mut sink = MockSink::<FrameH2>::new();

    frame_a
        .clone()
        .chain(frame_b.clone())
        .send_all(&mut sink)
        .await
        .expect("send chained frames");

    assert_eq!(sink.frames().len(), 2);
    assert_eq!(sink.frames()[0].payload, frame_a.payload);
    assert_eq!(sink.frames()[1].payload, frame_b.payload);
}

#[tokio::test]
async fn chain_batches_and_frames() {
    let frame = FrameH2::new(
        FrameTypeH2::WindowUpdate,
        0,
        1,
        Bytes::from_static(&[0, 0, 0, 1]),
    );

    let batch = FrameBatch::new(vec![frame.clone()]);
    let combined = batch.chain(frame.clone()).chain(frame.clone());

    let mut sink = MockSink::<FrameH2>::new();

    combined
        .send(&mut sink)
        .await
        .expect("send chained batch and frame");

    assert_eq!(sink.frames().len(), 3);
}

#[tokio::test]
async fn repeat_and_chain_h3_frames() {
    let headers_frame = FrameH3::new(FrameTypeH3::Headers, 0, Bytes::from_static(b"hdr"));
    let data_frame = FrameH3::new(FrameTypeH3::Data, 0, Bytes::from_static(b"dat"));

    let mut sink = MockSink::<FrameH3>::new();

    headers_frame
        .clone()
        .repeat(2)
        .chain(data_frame.clone())
        .send_all(&mut sink)
        .await
        .expect("send h3 frames");

    assert_eq!(sink.frames().len(), 3);
    assert!(matches!(
        sink.frames()[0].frame_type,
        FrameType::H3(FrameTypeH3::Headers)
    ));
    assert!(matches!(
        sink.frames()[2].frame_type,
        FrameType::H3(FrameTypeH3::Data)
    ));
}
