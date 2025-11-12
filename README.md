# riphttplib

riphttplib is a library specifically built for http security testing and exploit writing.

## Usage

add the package as git dependency to `Cargo.toml`, or download the library and use as path dependency:

```toml
[dependencies]
riphttplib = { git = "https://github.com/sebastianosrt/riphttplib" }
# or
# riphttplib = { path = "riphttplib" }
```

- client request:

```rust
use riphttplib::types::protocol::Client;
use riphttplib::H2; // or H1 / H3

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let response = Client::<H2>::post("https://httpbin.org/post")
        .query(vec![("test", "param")])
        .header("test: header".to_string())
        .data(vec![("test", "param")])
        .trailers(vec!["trailer: test".to_string()])
        .await?;

    println!("{}", response);
}
```

- explicit Request:

```rust
use riphttplib::{H1, Header, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut req = Request::new("https://www.example.com", "POST")?
            .header("te: trailers")
            .body("body")
            .trailer("test: trailer")
            .timeout(timeouts.clone())
            .follow_redirects(false));

    let client = H1::new(); // H2::new() or H3::new()
    let res = client.send_request(req).await?;
    println!("{}", res);
    Ok(())
}
```

- Frame API

HTTP2 Example

```rust
FrameH2::header(stream_id, &headers, end_stream, end_headers)?
    .chain(Frame::continuation(stream_id, &headers, end_stream)?.repeat(2))
    .chain(FrameH2::data(stream_id, &bytes, end_stream)?)
    .chain(...)
    .send(&mut connection)
    .await?;
```

- Connection handling

Use the `HttpConnection` trait for per‑connection control. Each protocol provides types and option structs:

- HTTP/1.1: `H1Connection` with `H1ConnectOptions`
- HTTP/2: `H2Connection` with `H2ConnectOptions`
- HTTP/3: `H3Connection` with `H3ConnectOptions`

HTTP/2 example:

```rust
use riphttplib::connection::HttpConnection;
use riphttplib::h2::connection::{H2ConnectOptions, H2Connection};
use riphttplib::{Request, types::ClientTimeouts, types::FrameH2};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://quic.tech";
    let req = Request::new(url, "GET")?;
    let headers = Request::prepare_pseudo_headers(&req)?;

    // create connection stream
    let mut conn = <H2Connection as HttpConnection>::connect(H2ConnectOptions {
        target: url.to_string(),
        timeouts: ClientTimeouts::disabled(),
    }).await?;

    // get stream id
    let stream_id = conn.create_stream().await?;
    
    // send header frame
    FrameH2::header(stream_id, &headers, true, true)?.send(&mut conn).await?;

    // read response from connection
    let res = <H2Connection as HttpConnection>::read_response(&mut conn, stream_id).await?;
    println!("{}", res);
    Ok(())
}
```

HTTP/3 example with custom event handler:

```rust
use riphttplib::connection::HttpConnection;
use riphttplib::h3::connection::{H3ConnectOptions, H3Connection};
use riphttplib::{Request, types::{ClientTimeouts, FrameH3, FrameType, FrameTypeH3}};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://quic.tech:8443";
    let req = Request::new(url, "GET")?;
    let headers = Request::prepare_pseudo_headers(&req)?;

    // create connection
    let mut conn = <H3Connection as HttpConnection>::connect(H3ConnectOptions {
        target: url.to_string(),
        timeouts: ClientTimeouts::disabled(),
    }).await?;
    let (stream_id, mut send) = conn.create_request_stream().await?;

    // create header frame
    let hb = conn.encode_headers(stream_id, &headers).await?; // qpack encode headers
    let serialized = FrameH3::header(stream_id, hb).serialize()?;
    
    // send frames
    send.write_all(&serialized).await?;
    send.finish()?;

    // custom event handler
    let printer = |frame: &FrameH3| match &frame.frame_type {
        FrameType::H3(FrameTypeH3::Headers) => println!("[{}] HEADERS", frame.stream_id),
        FrameType::H3(FrameTypeH3::Data) => println!("[{}] DATA ({} bytes)", frame.stream_id, frame.payload.len()),
        FrameType::H3(other) => println!("[{}] {:?}", frame.stream_id, other),
        _ => {}
    };

    let res = conn.read_response_with_timeouts(stream_id, &ClientTimeouts::disabled(), Some(&printer)).await?;
    println!("{}", res);
    Ok(())
}
```

See `examples/raw_h2.rs` and `examples/raw_h3.rs`.

- Sessions

Persist headers/cookies across multiple requests:

```rust
use riphttplib::H1;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = H1::new().session();
    session.header("user-agent: riphttplib");
    let res = session.get("https://httpbin.org/anything").send().await?;
    println!("{}", res);
    Ok(())
}
```

## Running the examples

```bash
cargo run --example simple_request
```
⚠️ examples contains DOS exploits, I do not take any responsibility for damages caused by the improper usage of this library.

## Collaborations

feel free to to open a pr or directly contact me


## TODOs

- [ ] documentation, for now check examples
- [ ] logging
- [ ] [single packet attack](https://portswigger.net/research/smashing-the-state-machine#single-packet-attack)
- [ ] [single packet attack++](https://flatt.tech/research/posts/beyond-the-limit-expanding-single-packet-race-condition-with-first-sequence-sync/)
- [ ] single datagram attack
- [ ] h1 last byte sync
- [ ] api for race conditions exploit writing
- [ ] python api [bindings](https://github.com/PyO3/pyo3)?
- [ ] custom quic impl https://www.imperva.com/blog/quic-leak-cve-2025-54939-new-high-risk-pre-handshake-remote-denial-of-service-in-lsquic-quic-implementation/ https://seemann.io/posts/2024-03-19---exploiting-quics-connection-id-management/
- [ ] protocol fuzzer
