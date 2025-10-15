use bytes::Bytes;
use riphttplib::h2::framing::{END_HEADERS_FLAG, END_STREAM_FLAG};
use riphttplib::types::{
    FrameBuilderExt, FrameH2, FrameH3, FrameType, FrameTypeH2, FrameTypeH3, Header,
};

fn roundtrip_h2(frame: &FrameH2) -> FrameH2 {
    let serialized = frame.serialize().expect("serialization succeeds");
    FrameH2::parse(&serialized).expect("parsing succeeds")
}

fn roundtrip_h3(frame: &FrameH3) -> FrameH3 {
    let serialized = frame.serialize().expect("serialize h3");
    let (parsed, _) = FrameH3::parse(&serialized).expect("parse h3");
    parsed
}

#[test]
fn h2_data_roundtrip() {
    let frame = FrameH2::data(3, Bytes::from_static(b"hello"), true);
    let parsed = roundtrip_h2(&frame);
    assert!(matches!(
        parsed.frame_type,
        FrameType::H2(FrameTypeH2::Data)
    ));
    assert_eq!(parsed.stream_id, 3);
    assert!(parsed.is_end_stream());
    assert_eq!(parsed.payload, Bytes::from_static(b"hello"));
}

#[test]
fn h2_headers_roundtrip() {
    // HPACK payload bytes (pretend)
    let headers = vec![Header::new(":method".into(), "GET".into())];
    let frame = FrameH2::headers(1, &headers, true, true).expect("encode headers");
    let parsed = roundtrip_h2(&frame);
    assert!(matches!(
        parsed.frame_type,
        FrameType::H2(FrameTypeH2::Headers)
    ));
    assert_eq!(parsed.flags & END_STREAM_FLAG, END_STREAM_FLAG);
    assert_eq!(parsed.stream_id, 1);
    assert!(!parsed.payload.is_empty());
}

#[test]
fn h2_continuation_roundtrip() {
    let block = Bytes::from_static(&[0, 1, 2, 3]);
    let frame = FrameH2::new(
        FrameTypeH2::Continuation,
        END_HEADERS_FLAG,
        5,
        block.clone(),
    );
    let parsed = roundtrip_h2(&frame);
    assert!(matches!(
        parsed.frame_type,
        FrameType::H2(FrameTypeH2::Continuation)
    ));
    assert_eq!(parsed.stream_id, 5);
    assert_eq!(parsed.payload, block);
    assert!(parsed.is_end_headers());
}

#[test]
fn h3_roundtrip() {
    let frame = FrameH3::new(FrameTypeH3::Data, 7, Bytes::from_static(b"payload"));
    let parsed = roundtrip_h3(&frame);
    assert!(matches!(
        parsed.frame_type,
        FrameType::H3(FrameTypeH3::Data)
    ));
    // Parsing assigns stream_id = 0 because QUIC tracks it externally.
    assert_eq!(parsed.stream_id, 0);
    assert_eq!(parsed.payload, Bytes::from_static(b"payload"));
}

#[test]
fn frame_batch_chain_serializes_all() {
    let f1 = FrameH2::data(1, Bytes::from_static(b"a"), false);
    let f2 = FrameH2::data(1, Bytes::from_static(b"b"), true);

    let batch = f1.clone().chain(f2.clone());
    assert_eq!(batch.frames().len(), 2);
    let seq: Vec<_> = batch.into_frames();
    assert_eq!(seq[0].payload, f1.payload);
    assert_eq!(seq[1].payload, f2.payload);
}
