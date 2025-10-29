use riphttplib::{H1Client, Header};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = H1Client::new().session();

    session.add_default_header(Header::new(
        "user-agent".into(),
        "riphttplib-session-example/0.1".into(),
    ));
    session.add_default_header(Header::new("accept".into(), "text/html".into()));
    session.set_cookie("example-cookie", "hello-world");

    let first_response = session
        .get(
            "https://quic.tech:8443",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
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

    let second_response = session
        .get(
            "https://quic.tech:8443",
            Some(vec![Header::new("accept-language".into(), "en-US".into())]),
            None,
            None,
            Some(vec![("demo".to_string(), "1".to_string())]),
            Some(true),
            None,
            None,
            None,
        )
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
