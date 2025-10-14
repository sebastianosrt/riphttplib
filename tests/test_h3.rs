use riphttplib::h3::H3Client;
use riphttplib::types::Request;
use riphttplib::utils::parse_target;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_http3_google() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let client = H3Client::new();
    let target = parse_target("https://www.google.com/").expect("valid target");

    let request = Request::new("GET");
    match client.send_request(&target, request).await {
        Ok(response) => {
            println!("Status: {}", response.status);
            for header in &response.headers {
                if let Some(ref value) = header.value {
                    println!("{}: {}", header.name, value);
                } else {
                    println!("{}", header.name);
                }
            }
            println!("Body length: {}", response.body.len());

            assert!(response.status >= 200 && response.status < 400);
            assert!(!response.headers.is_empty());
        }
        Err(err) => {
            eprintln!("HTTP/3 request failed: {}", err);
        }
    }
}
