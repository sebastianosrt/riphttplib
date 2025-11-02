use riphttplib::types::ClientTimeouts;
use riphttplib::{Header, H1};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = H1::new().session();

    session.header(Header::new(
        "user-agent".into(),
        "riphttplib-session-example/0.2".into(),
    ));
    session.header(Header::new("accept".into(), "text/html".into()));
    session.set_cookie("example-cookie", "hello-world");

    let first_response = session
        .get("https://httpbin.org/anything/session")
        .query(vec![("first", "true")])
        .send()
        .await?;
    println!(
        "First response: {} {} (frames: {})",
        first_response.protocol,
        first_response.status,
        first_response.frames.as_ref().map(|f| f.len()).unwrap_or(0)
    );
    if !first_response.cookies.is_empty() {
        println!("  cookies: {:?}", first_response.cookies);
    }

    if let Some(frames) = &first_response.frames {
        println!("  captured {} frame(s)", frames.len());
    }

    let timeouts = ClientTimeouts {
        connect: Some(std::time::Duration::from_secs(5)),
        read: Some(std::time::Duration::from_secs(10)),
        write: Some(std::time::Duration::from_secs(5)),
    };

    let second_response = session
        .get("https://httpbin.org/anything/session")
        .header_value(Header::new("accept-language".into(), "en-US".into()))
        .cookies(vec![("demo", "1")])
        .allow_redirects(true)
        .timeout(timeouts)
        .send()
        .await?;
    println!(
        "Second response: {} {} (frames: {})",
        second_response.protocol,
        second_response.status,
        second_response
            .frames
            .as_ref()
            .map(|f| f.len())
            .unwrap_or(0)
    );
    if !second_response.cookies.is_empty() {
        println!("  cookies: {:?}", second_response.cookies);
    }

    println!("Session cookies: {}", session.cookies);

    Ok(())
}
