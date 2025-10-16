# riphttplib

What is this?

features:
- http1.1/2/3 client library with trailers support
- no strict validation, freedom to build any malformed frame and request
- easy interface for frames building

future implementations:
- h2 single packet attack (h2SPA) https://portswigger.net/research/smashing-the-state-machine#single-packet-attack https://github.com/nxenon/h2spacex
- h2SPA++ https://flatt.tech/research/posts/beyond-the-limit-expanding-single-packet-race-condition-with-first-sequence-sync/
- h1 last byte sync
- think about features for request smuggling + pause based desync

feel free to suggest me interesting features

## Collaborations

feel free to open a pr or directly contact me

## Quick Start

```rust
use riphttplib::h1::H1Client;
use riphttplib::types::{Request, Header};
use riphttplib::utils::parse_target;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let client = H1Client::new();
let target = parse_target("http://example.com/").expect("valid url");

let request = Request::new("GET")
    .with_headers(vec![Header::new("accept".into(), "text/plain".into())]);

let response = client.send_request(&target, request).await?;
println!("status: {}", response.status);
# Ok(())
# }
```
