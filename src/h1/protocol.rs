use crate::stream::{create_stream, TransportStream};
use crate::types::{ClientTimeouts, Header, Protocol, ProtocolError, Request, Response};
use crate::utils::{
    timeout_result, CHUNKED_ENCODING, CONTENT_LENGTH_HEADER, CRLF, HOST_HEADER, HTTP_VERSION_1_1,
    TRANSFER_ENCODING_HEADER,
};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Clone)]
pub struct H1 {
    timeouts: ClientTimeouts,
}

impl H1 {
    pub fn new() -> Self {
        Self::timeouts(ClientTimeouts::default())
    }

    pub fn timeouts(timeouts: ClientTimeouts) -> Self {
        Self { timeouts }
    }

    pub fn get_timeouts(&self) -> &ClientTimeouts {
        &self.timeouts
    }

    pub fn session(&self) -> crate::session::H1Session {
        crate::session::H1Session::new(self.clone())
    }

    pub async fn send_request(&self, request: Request) -> Result<Response, ProtocolError> {
        <Self as Protocol>::response(self, request).await
    }

    async fn perform_request(&self, request: &Request) -> Result<Response, ProtocolError> {
        let timeouts = request.timeouts(&self.timeouts);
        let mut stream = self.open_stream(request, &timeouts).await?;
        self.write_request(&mut stream, request, &timeouts).await?;
        self.read_response(&mut stream, &request.method, &timeouts)
            .await
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
                return timeout_result(connect_timeout, async move {
                    if target.scheme() == "https" {
                        crate::proxy::connect_through_proxy_https(
                            socks_proxy,
                            host,
                            port,
                            connect_timeout,
                        )
                        .await
                    } else {
                        crate::proxy::connect_through_proxy(
                            socks_proxy,
                            host,
                            port,
                            connect_timeout,
                        )
                        .await
                    }
                })
                .await;
            }

            // Then check for HTTP/HTTPS proxy
            let proxy_url = if target.scheme() == "https" {
                proxy_settings
                    .https
                    .as_ref()
                    .or(proxy_settings.http.as_ref())
            } else {
                proxy_settings.http.as_ref()
            };

            if let Some(proxy_url) = proxy_url {
                let proxy_config = if target.scheme() == "https" {
                    crate::types::ProxyConfig::https(proxy_url.clone())
                } else {
                    crate::types::ProxyConfig::http(proxy_url.clone())
                };

                return timeout_result(connect_timeout, async move {
                    if target.scheme() == "https" {
                        crate::proxy::connect_through_proxy_https(
                            &proxy_config,
                            host,
                            port,
                            connect_timeout,
                        )
                        .await
                    } else {
                        crate::proxy::connect_through_proxy(
                            &proxy_config,
                            host,
                            port,
                            connect_timeout,
                        )
                        .await
                    }
                })
                .await;
            }
        }

        // Direct connection
        let host_owned = host.to_string();
        timeout_result(connect_timeout, async move {
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

        let mut headers = request.prepare_headers();
        // TODO add connection header
        let trailers = request.trailers.clone();

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
        timeout_result(write_timeout, async {
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
            let bytes = timeout_result(timeouts.read, async {
                match reader.read_line(&mut status_line).await {
                    Ok(bytes) => Ok(bytes),
                    Err(e) => {
                        // Handle UTF-8 and TLS close_notify errors gracefully
                        if let Some(custom_error) = e.get_ref() {
                            let error_msg = custom_error.to_string();
                            if error_msg
                                .contains("peer closed connection without sending TLS close_notify")
                                || error_msg.contains("stream did not contain valid UTF-8")
                            {
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

            let (body, trailers) = if !Self::response_has_body(method, status) {
                (Bytes::new(), Vec::new())
            } else { 
                self.read_body(reader, &headers, timeouts).await?
            };

            let cookies = Response::collect_cookies(&headers);

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
                frames: None,
                cookies,
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
            match timeout_result(timeouts.read, async {
                match reader.read_line(&mut line).await {
                    Ok(bytes) => Ok(bytes),
                    Err(e) => {
                        // Handle UTF-8 and TLS close_notify errors gracefully
                        if let Some(custom_error) = e.get_ref() {
                            let error_msg = custom_error.to_string();
                            if error_msg
                                .contains("peer closed connection without sending TLS close_notify")
                                || error_msg.contains("stream did not contain valid UTF-8")
                            {
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
            .await
            {
                Ok(0) => break, // EOF or error treated as EOF
                Ok(_) => {}
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
        timeouts: &ClientTimeouts,
    ) -> Result<(Bytes, Vec<Header>), ProtocolError> {
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
                timeout_result(timeouts.read, async {
                    reader
                        .read_exact(&mut body)
                        .await
                        .map_err(ProtocolError::Io)
                })
                .await?;
                Ok((Bytes::from(body), Vec::new()))
            } else {
                let mut body = Vec::new();
                let _result = timeout_result(timeouts.read, async {
                    // Use a custom reading loop to handle TLS close_notify gracefully
                    loop {
                        let mut buffer = [0u8; 8192];
                        match reader.read(&mut buffer).await {
                            Ok(0) => break, // Normal EOF
                            Ok(n) => body.extend_from_slice(&buffer[..n]),
                            Err(e) => {
                                // Handle TLS close_notify issue gracefully
                                if let Some(custom_error) = e.get_ref() {
                                    if custom_error.to_string().contains(
                                        "peer closed connection without sending TLS close_notify",
                                    ) {
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
            timeout_result(timeouts.read, async {
                match reader.read_line(&mut size_line).await {
                    Ok(bytes) => Ok(bytes),
                    Err(e) => {
                        // Handle UTF-8 and TLS close_notify errors gracefully
                        if let Some(custom_error) = e.get_ref() {
                            let error_msg = custom_error.to_string();
                            if error_msg
                                .contains("peer closed connection without sending TLS close_notify")
                                || error_msg.contains("stream did not contain valid UTF-8")
                            {
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
                    timeout_result(timeouts.read, async {
                        match reader.read_line(&mut line).await {
                            Ok(bytes) => Ok(bytes),
                            Err(e) => {
                                // Handle UTF-8 and TLS close_notify errors gracefully
                                if let Some(custom_error) = e.get_ref() {
                                    let error_msg = custom_error.to_string();
                                    if error_msg.contains(
                                        "peer closed connection without sending TLS close_notify",
                                    ) || error_msg.contains("stream did not contain valid UTF-8")
                                    {
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
            timeout_result(timeouts.read, async {
                reader
                    .read_exact(&mut chunk)
                    .await
                    .map_err(ProtocolError::Io)
            })
            .await?;
            body.extend_from_slice(&chunk);

            let mut crlf = [0u8; 2];
            timeout_result(timeouts.read, async {
                reader
                    .read_exact(&mut crlf)
                    .await
                    .map_err(ProtocolError::Io)
            })
            .await?;
        }

        Ok((Bytes::from(body), trailers))
    }

    // TODO write better
    fn response_has_body(method: &str, status: u16) -> bool {
        if method.eq_ignore_ascii_case("HEAD") || (100..200).contains(&status) {
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
            let mut trailer_line = format!("{}{}", trailer.to_string(), CRLF);
            if matches!(trailer.to_string().as_str(), "\n" | "\r" | "\r\n") {
                trailer_line = trailer.to_string();
            }
            chunked_body.extend_from_slice(trailer_line.as_bytes());
        }

        chunked_body.extend_from_slice(CRLF.as_bytes());

        chunked_body
    }

    pub fn parse_status_line(status_line: &str) -> Result<(u16, String), ProtocolError> {
        // TODO what if it is HTTP/0.9
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
}

#[async_trait(?Send)]
impl Protocol for H1 {
    async fn execute(&self, request: &Request) -> Result<Response, ProtocolError> {
        self.perform_request(request).await
    }

    async fn send_raw(&self, target: &str, raw_request: Bytes) -> Result<Response, ProtocolError> {
        if raw_request.is_empty() {
            return Err(ProtocolError::RequestFailed(
                "Raw request payload cannot be empty".to_string(),
            ));
        }

        let request = Request::new(target, "RAW")?;
        let timeouts = request.timeouts(&self.timeouts);
        let mut stream = self.open_stream(&request, &timeouts).await?;

        self.write_to_stream(&mut stream, raw_request.as_ref(), timeouts.write)
            .await?;

        self.read_response(&mut stream, "RAW", &timeouts).await
    }
}
