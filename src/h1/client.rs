use crate::stream::{create_stream, TransportStream};
use crate::types::{ClientTimeouts, Header, Protocol, ProtocolError, Request, Response};
use crate::utils::{
    ensure_user_agent, with_timeout_result, CHUNKED_ENCODING, CONTENT_LENGTH_HEADER, CRLF,
    HOST_HEADER, HTTP_VERSION_1_1, TRANSFER_ENCODING_HEADER, parse_target,
};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use url::Url;

pub struct H1Client {
    timeouts: ClientTimeouts,
}

impl H1Client {
    pub fn new() -> Self {
        Self::with_timeouts(ClientTimeouts::default())
    }

    pub fn with_timeouts(timeouts: ClientTimeouts) -> Self {
        Self { timeouts }
    }

    pub fn timeouts(&self) -> &ClientTimeouts {
        &self.timeouts
    }

    pub async fn send_request(&self, request: Request) -> Result<Response, ProtocolError> {
        self.send_request_internal(request, 0).await
    }

    fn send_request_internal<'a>(&'a self, mut request: Request, redirect_count: u32) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, ProtocolError>> + 'a>> {
        Box::pin(async move {
            const MAX_REDIRECTS: u32 = 30;

            if redirect_count > MAX_REDIRECTS {
                return Err(ProtocolError::RequestFailed("Too many redirects".to_string()));
            }

            let timeouts = request.effective_timeouts(&self.timeouts);
            let mut stream = self.open_stream(&request, &timeouts).await?;
            self.write_request(&mut stream, &request, &timeouts).await?;

            let response = self.read_response(&mut stream, &request.method, &timeouts).await?;

            // Handle redirects if enabled
            if request.allow_redirects && Self::is_redirect_status(response.status) {
                if let Some(location) = Self::get_location_header(&response.headers) {
                    if let Ok(redirect_url) = Self::resolve_redirect_url(&request.target.url, &location) {
                        let new_target = parse_target(redirect_url.as_str())?;
                        request.target = new_target;

                        // For 303 responses or GET/HEAD methods on 301/302, change method to GET
                        if response.status == 303 ||
                           ((response.status == 301 || response.status == 302) &&
                            (request.method == "GET" || request.method == "HEAD")) {
                            request.method = "GET".to_string();
                            request.body = None;
                            request.json = None;
                        }

                        return self.send_request_internal(request, redirect_count + 1).await;
                    }
                }
            }

            Ok(response)
        })
    }

    async fn open_stream(
        &self,
        request: &Request,
        timeouts: &ClientTimeouts,
    ) -> Result<TransportStream, ProtocolError> {
        let target = &request.target;
        let host = target
            .host()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing host".to_string()))?;
        let port = target
            .port()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing port".to_string()))?;

        let scheme = target.scheme().to_string();
        let connect_timeout = timeouts.connect;

        // Handle proxy if configured
        if let Some(proxy_settings) = &request.proxies {
            // First check for SOCKS proxy
            if let Some(socks_proxy) = &proxy_settings.socks {
                return with_timeout_result(connect_timeout, async move {
                    if target.scheme() == "https" {
                        crate::proxy::connect_through_proxy_https(socks_proxy, host, port, connect_timeout).await
                    } else {
                        crate::proxy::connect_through_proxy(socks_proxy, host, port, connect_timeout).await
                    }
                })
                .await;
            }

            // Then check for HTTP/HTTPS proxy
            let proxy_url = if target.scheme() == "https" {
                proxy_settings.https.as_ref().or(proxy_settings.http.as_ref())
            } else {
                proxy_settings.http.as_ref()
            };

            if let Some(proxy_url) = proxy_url {
                let proxy_config = if target.scheme() == "https" {
                    crate::types::ProxyConfig::https(proxy_url.clone())
                } else {
                    crate::types::ProxyConfig::http(proxy_url.clone())
                };

                return with_timeout_result(connect_timeout, async move {
                    if target.scheme() == "https" {
                        crate::proxy::connect_through_proxy_https(&proxy_config, host, port, connect_timeout).await
                    } else {
                        crate::proxy::connect_through_proxy(&proxy_config, host, port, connect_timeout).await
                    }
                })
                .await;
            }
        }

        // Direct connection
        let host_owned = host.to_string();
        with_timeout_result(connect_timeout, async move {
            create_stream(&scheme, &host_owned, port, connect_timeout)
                .await
                .map_err(|e| ProtocolError::ConnectionFailed(e.to_string()))
        })
        .await
    }

    async fn write_request(
        &self,
        stream: &mut TransportStream,
        request: &Request,
        timeouts: &ClientTimeouts,
    ) -> Result<(), ProtocolError> {
        let mut req = Vec::new();
        let path = request.path();

        let empty_trailers = Vec::new();
        let mut headers = request.effective_headers();
        let trailers = request.trailers.as_ref().unwrap_or(&empty_trailers);
        ensure_user_agent(&mut headers);

        req.extend_from_slice(
            format!("{} {} {}{}", request.method, path, HTTP_VERSION_1_1, CRLF).as_bytes(),
        );

        let has_host = headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(HOST_HEADER));
        if !has_host {
            let authority = request
                .target
                .authority()
                .unwrap_or_else(|| request.target.host().unwrap_or_default().to_string());
            headers.push(Header::new(HOST_HEADER.to_string(), authority));
        }

        let has_content_length = headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(CONTENT_LENGTH_HEADER));
        let has_chunked = headers.iter().any(|h| {
            h.name.eq_ignore_ascii_case(TRANSFER_ENCODING_HEADER)
                && h.value
                    .as_ref()
                    .map_or(false, |v| v.to_lowercase().contains(CHUNKED_ENCODING))
        });

        let use_chunked = has_chunked || !trailers.is_empty();
        let body_len = request.body.as_ref().map(|b| b.len());
        let should_generate_content_length =
            body_len.is_some() && !use_chunked && !has_content_length;

        if should_generate_content_length {
            headers.push(Header::new(
                CONTENT_LENGTH_HEADER.to_string(),
                body_len.unwrap().to_string(),
            ));
        } else if use_chunked && !has_chunked {
            headers.push(Header::new(
                TRANSFER_ENCODING_HEADER.to_string(),
                CHUNKED_ENCODING.to_string(),
            ));
        }

        for header in &headers {
            req.extend_from_slice(format!("{}{}", header.to_string(), CRLF).as_bytes());
        }

        req.extend_from_slice(CRLF.as_bytes());

        let empty_body = Bytes::new();
        if use_chunked {
            let body = request.body.as_ref().unwrap_or(&empty_body);
            let chunked_body = Self::build_chunked_body(body, trailers.as_slice());
            req.extend_from_slice(&chunked_body);
        } else if let Some(body) = request.body.as_ref() {
            req.extend_from_slice(body);
        }

        self.write_to_stream(stream, &req, timeouts.write).await
    }

    async fn write_to_stream(
        &self,
        stream: &mut TransportStream,
        data: &[u8],
        write_timeout: Option<std::time::Duration>,
    ) -> Result<(), ProtocolError> {
        with_timeout_result(write_timeout, async {
            match stream {
                TransportStream::Tcp(tcp) => tcp.write_all(data).await.map_err(ProtocolError::Io),
                TransportStream::Tls(tls) => tls.write_all(data).await.map_err(ProtocolError::Io),
            }
        })
        .await
    }

    async fn read_response(
        &self,
        stream: &mut TransportStream,
        method: &str,
        timeouts: &ClientTimeouts,
    ) -> Result<Response, ProtocolError> {
        match stream {
            TransportStream::Tcp(tcp) => {
                let mut reader = BufReader::new(tcp);
                self.read_response_from_reader(&mut reader, method, timeouts)
                    .await
            }
            TransportStream::Tls(tls) => {
                let mut reader = BufReader::new(tls);
                self.read_response_from_reader(&mut reader, method, timeouts)
                    .await
            }
        }
    }

    async fn read_response_from_reader<R: AsyncBufRead + Unpin>(
        &self,
        reader: &mut R,
        method: &str,
        timeouts: &ClientTimeouts,
    ) -> Result<Response, ProtocolError> {
        loop {
            let mut status_line = String::new();
            let bytes = with_timeout_result(timeouts.read, async {
                match reader.read_line(&mut status_line).await {
                    Ok(bytes) => Ok(bytes),
                    Err(e) => {
                        // Handle UTF-8 and TLS close_notify errors gracefully
                        if let Some(custom_error) = e.get_ref() {
                            let error_msg = custom_error.to_string();
                            if error_msg.contains("peer closed connection without sending TLS close_notify") ||
                               error_msg.contains("stream did not contain valid UTF-8") {
                                return Ok(0); // Treat as EOF
                            }
                        }
                        if e.kind() == std::io::ErrorKind::InvalidData {
                            return Ok(0); // Treat UTF-8 errors as EOF
                        }
                        Err(ProtocolError::Io(e))
                    }
                }
            })
            .await?;

            if bytes == 0 {
                // If we get EOF on the first read, it might be due to TLS close_notify issue
                // Try to continue with an empty response or return a more specific error
                return Err(ProtocolError::ConnectionFailed(
                    "Connection closed by server before receiving response".to_string(),
                ));
            }

            if status_line.trim().is_empty() {
                continue;
            }

            let (status, protocol) = Self::parse_status_line(&status_line)?;
            let headers = self.read_header_block(reader, timeouts).await?;

            if status < 200 {
                // 101 Switching Protocols is a final response (RFC 7231 §6.2.2)
                if status != 101 {
                    continue;
                }
            }

            let (body, trailers) =
                self.read_body(reader, &headers, method, status, timeouts).await?;

            return Ok(Response {
                status,
                protocol,
                headers,
                body,
                trailers: if trailers.is_empty() {
                    None
                } else {
                    Some(trailers)
                },
            });
        }
    }

    async fn read_header_block<R: AsyncBufRead + Unpin>(
        &self,
        reader: &mut R,
        timeouts: &ClientTimeouts,
    ) -> Result<Vec<Header>, ProtocolError> {
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            match with_timeout_result(timeouts.read, async {
                match reader.read_line(&mut line).await {
                    Ok(bytes) => Ok(bytes),
                    Err(e) => {
                        // Handle UTF-8 and TLS close_notify errors gracefully
                        if let Some(custom_error) = e.get_ref() {
                            let error_msg = custom_error.to_string();
                            if error_msg.contains("peer closed connection without sending TLS close_notify") ||
                               error_msg.contains("stream did not contain valid UTF-8") {
                                return Ok(0); // Treat as EOF
                            }
                        }
                        if e.kind() == std::io::ErrorKind::InvalidData {
                            return Ok(0); // Treat UTF-8 errors as EOF
                        }
                        Err(ProtocolError::Io(e))
                    }
                }
            })
            .await {
                Ok(0) => break, // EOF or error treated as EOF
                Ok(_) => {},
                Err(e) => return Err(e),
            }

            if line.trim().is_empty() {
                break;
            }

            if let Some(header) = crate::utils::parse_header(line.trim()) {
                headers.push(header);
            }
        }
        Ok(headers)
    }

    async fn read_body<R: AsyncBufRead + Unpin>(
        &self,
        reader: &mut R,
        headers: &[Header],
        method: &str,
        status: u16,
        timeouts: &ClientTimeouts,
    ) -> Result<(Bytes, Vec<Header>), ProtocolError> {
        if !Self::response_has_body(method, status) {
            return Ok((Bytes::new(), Vec::new()));
        }

        let is_chunked = headers.iter().any(|h| {
            h.name.to_lowercase() == TRANSFER_ENCODING_HEADER
                && h.value
                    .as_ref()
                    .map_or(false, |v| v.to_lowercase().contains(CHUNKED_ENCODING))
        });

        if is_chunked {
            self.read_chunked_body(reader, timeouts).await
        } else {
            let content_length = headers
                .iter()
                .find(|h| h.name.to_lowercase() == CONTENT_LENGTH_HEADER)
                .and_then(|h| h.value.as_ref())
                .and_then(|v| v.parse::<usize>().ok());

            if let Some(length) = content_length {
                let mut body = vec![0u8; length];
                with_timeout_result(timeouts.read, async {
                    reader
                        .read_exact(&mut body)
                        .await
                        .map_err(ProtocolError::Io)
                })
                .await?;
                Ok((Bytes::from(body), Vec::new()))
            } else {
                let mut body = Vec::new();
                let _result = with_timeout_result(timeouts.read, async {
                    // Use a custom reading loop to handle TLS close_notify gracefully
                    loop {
                        let mut buffer = [0u8; 8192];
                        match reader.read(&mut buffer).await {
                            Ok(0) => break, // Normal EOF
                            Ok(n) => body.extend_from_slice(&buffer[..n]),
                            Err(e) => {
                                // Handle TLS close_notify issue gracefully
                                if let Some(custom_error) = e.get_ref() {
                                    if custom_error.to_string().contains("peer closed connection without sending TLS close_notify") {
                                        // This is a common occurrence with HTTPS servers, treat as successful EOF
                                        break;
                                    }
                                }
                                return Err(ProtocolError::Io(e));
                            }
                        }
                    }
                    Ok(())
                })
                .await?;
                Ok((Bytes::from(body), Vec::new()))
            }
        }
    }

    async fn read_chunked_body<R: AsyncBufRead + Unpin>(
        &self,
        reader: &mut R,
        timeouts: &ClientTimeouts,
    ) -> Result<(Bytes, Vec<Header>), ProtocolError> {
        let mut body = Vec::new();
        let mut trailers = Vec::new();

        loop {
            let mut size_line = String::new();
            with_timeout_result(timeouts.read, async {
                match reader.read_line(&mut size_line).await {
                    Ok(bytes) => Ok(bytes),
                    Err(e) => {
                        // Handle UTF-8 and TLS close_notify errors gracefully
                        if let Some(custom_error) = e.get_ref() {
                            let error_msg = custom_error.to_string();
                            if error_msg.contains("peer closed connection without sending TLS close_notify") ||
                               error_msg.contains("stream did not contain valid UTF-8") {
                                return Ok(0); // Treat as EOF
                            }
                        }
                        if e.kind() == std::io::ErrorKind::InvalidData {
                            return Ok(0); // Treat UTF-8 errors as EOF
                        }
                        Err(ProtocolError::Io(e))
                    }
                }
            })
            .await?;

            let size_str = size_line.trim().split(';').next().unwrap_or(" ").trim();
            let chunk_size = usize::from_str_radix(size_str, 16)
                .map_err(|_| ProtocolError::InvalidResponse("Invalid chunk size".to_string()))?;

            if chunk_size == 0 {
                loop {
                    let mut line = String::new();
                    with_timeout_result(timeouts.read, async {
                        match reader.read_line(&mut line).await {
                            Ok(bytes) => Ok(bytes),
                            Err(e) => {
                                // Handle UTF-8 and TLS close_notify errors gracefully
                                if let Some(custom_error) = e.get_ref() {
                                    let error_msg = custom_error.to_string();
                                    if error_msg.contains("peer closed connection without sending TLS close_notify") ||
                                       error_msg.contains("stream did not contain valid UTF-8") {
                                        return Ok(0); // Treat as EOF
                                    }
                                }
                                if e.kind() == std::io::ErrorKind::InvalidData {
                                    return Ok(0); // Treat UTF-8 errors as EOF
                                }
                                Err(ProtocolError::Io(e))
                            }
                        }
                    })
                    .await?;

                    if line.trim().is_empty() {
                        break;
                    }

                    if let Some(trailer) = crate::utils::parse_header(line.trim()) {
                        trailers.push(trailer);
                    }
                }
                break;
            }

            let mut chunk = vec![0u8; chunk_size];
            with_timeout_result(timeouts.read, async {
                reader
                    .read_exact(&mut chunk)
                    .await
                    .map_err(ProtocolError::Io)
            })
            .await?;
            body.extend_from_slice(&chunk);

            let mut crlf = [0u8; 2];
            with_timeout_result(timeouts.read, async {
                reader
                    .read_exact(&mut crlf)
                    .await
                    .map_err(ProtocolError::Io)
            })
            .await?;
        }

        Ok((Bytes::from(body), trailers))
    }

    fn response_has_body(method: &str, status: u16) -> bool {
        if method.eq_ignore_ascii_case("HEAD") {
            return false;
        }

        if (100..200).contains(&status) {
            return false;
        }

        !matches!(status, 204 | 205 | 304)
    }

    fn build_chunked_body(body: &Bytes, trailers: &[Header]) -> Vec<u8> {
        let mut chunked_body = Vec::new();

        if !body.is_empty() {
            let chunk_size = format!("{:x}{}", body.len(), CRLF);
            chunked_body.extend_from_slice(chunk_size.as_bytes());
            chunked_body.extend_from_slice(body);
            chunked_body.extend_from_slice(CRLF.as_bytes());
        }

        let final_chunk = format!("0{}", CRLF);
        chunked_body.extend_from_slice(final_chunk.as_bytes());

        for trailer in trailers {
            let trailer_line = format!("{}{}", trailer.to_string(), CRLF);
            chunked_body.extend_from_slice(trailer_line.as_bytes());
        }

        chunked_body.extend_from_slice(CRLF.as_bytes());

        chunked_body
    }

    pub fn parse_status_line(status_line: &str) -> Result<(u16, String), ProtocolError> {
        let parts: Vec<&str> = status_line.trim().split_whitespace().collect();
        if parts.len() < 2 {
            return Err(ProtocolError::InvalidResponse(
                "Invalid status line".to_string(),
            ));
        }

        let protocol = parts[0].to_string();
        let status_code = parts[1]
            .parse::<u16>()
            .map_err(|_| ProtocolError::InvalidResponse("Invalid status code".to_string()))?;

        Ok((status_code, protocol))
    }

    fn is_redirect_status(status: u16) -> bool {
        matches!(status, 300..=399)
    }

    fn get_location_header(headers: &[Header]) -> Option<String> {
        headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("location"))
            .and_then(|h| h.value.clone())
    }

    fn resolve_redirect_url(base_url: &Url, location: &str) -> Result<Url, url::ParseError> {
        if location.starts_with("http://") || location.starts_with("https://") {
            Url::parse(location)
        } else {
            base_url.join(location)
        }
    }
}

#[async_trait(?Send)]
impl Protocol for H1Client {
    async fn send(&self, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(request).await
    }
}
