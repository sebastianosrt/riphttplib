use crate::stream::{create_stream, TransportStream};
use crate::types::{Header, Protocol, ProtocolError, Request, Response, Target};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub struct H1Client;

// HTTP/1.1 Constants
static CRLF: &str = "\r\n";
static USER_AGENT: &str = "riphttplib/0.1.0";
static HTTP_VERSION: &str = "HTTP/1.1";
static HOST_HEADER: &str = "host";
static CONTENT_LENGTH_HEADER: &str = "content-length";
static TRANSFER_ENCODING_HEADER: &str = "transfer-encoding";
static USER_AGENT_HEADER: &str = "user-agent";
static CHUNKED_ENCODING: &str = "chunked";

impl H1Client {
    pub fn new() -> Self {
        H1Client
    }

    pub async fn send_request(
        &self,
        target: &Target,
        request: Request,
    ) -> Result<Response, ProtocolError> {
        let host = target
            .host()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing host".to_string()))?;
        let port = target
            .port()
            .ok_or_else(|| ProtocolError::InvalidTarget("Target missing port".to_string()))?;

        let mut stream = create_stream(target.scheme(), host, port)
            .await
            .map_err(|e| ProtocolError::ConnectionFailed(e.to_string()))?;

        Self::write_request(&mut stream, target, &request).await?;
        Self::read_response(&mut stream, &request.method).await
    }

    // TODO: write to stream or buffer and write all?
    async fn write_request(
        stream: &mut TransportStream,
        target: &Target,
        request: &Request,
    ) -> Result<(), ProtocolError> {
        let path = target.path();

        let request_line = format!("{} {} {}{}", request.method, path, HTTP_VERSION, CRLF);
        Self::write_to_stream(stream, request_line.as_bytes()).await?;

        let empty_trailers = Vec::new();
        let headers = &request.headers;
        let trailers = request.trailers.as_ref().unwrap_or(&empty_trailers);

        let has_host = headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(HOST_HEADER));
        if !has_host {
            let authority = target
                .authority()
                .unwrap_or_else(|| target.host().unwrap_or_default().to_string());
            let host_header = format!("Host: {}{}", authority, CRLF);
            Self::write_to_stream(stream, host_header.as_bytes()).await?;
        }

        let has_user_agent = headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(USER_AGENT_HEADER));
        if !has_user_agent {
            let user_agent_header = format!("User-Agent: {}{}", USER_AGENT, CRLF);
            Self::write_to_stream(stream, user_agent_header.as_bytes()).await?;
        }

        for header in headers {
            let header_line = format!("{}{}", header.to_string(), CRLF);
            Self::write_to_stream(stream, header_line.as_bytes()).await?;
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
            let length_header = format!("Content-Length: {}{}", body_len.unwrap(), CRLF);
            Self::write_to_stream(stream, length_header.as_bytes()).await?;
        } else if use_chunked && !has_chunked {
            // Add Transfer-Encoding: chunked only when we must synthesize it.
            let chunked_header = format!("Transfer-Encoding: {}{}", CHUNKED_ENCODING, CRLF);
            Self::write_to_stream(stream, chunked_header.as_bytes()).await?;
        }

        // End headers
        Self::write_to_stream(stream, CRLF.as_bytes()).await?;

        // Write body and trailers
        let empty_body = Bytes::new();
        if use_chunked {
            let body = request.body.as_ref().unwrap_or(&empty_body);
            Self::write_chunked_body(stream, body, trailers.as_slice()).await?;
        } else if let Some(body) = request.body.as_ref() {
            Self::write_to_stream(stream, body).await?;
        }

        Ok(())
    }

    async fn write_to_stream(
        stream: &mut TransportStream,
        data: &[u8],
    ) -> Result<(), ProtocolError> {
        match stream {
            TransportStream::Tcp(tcp) => tcp.write_all(data).await.map_err(ProtocolError::Io)?,
            TransportStream::Tls(tls) => tls.write_all(data).await.map_err(ProtocolError::Io)?,
        }
        Ok(())
    }

    async fn write_chunked_body(
        stream: &mut TransportStream,
        body: &Bytes,
        trailers: &[Header],
    ) -> Result<(), ProtocolError> {
        if !body.is_empty() {
            // Write chunk size in hex
            let chunk_size = format!("{:x}{}", body.len(), CRLF);
            Self::write_to_stream(stream, chunk_size.as_bytes()).await?;

            // Write chunk data
            Self::write_to_stream(stream, body).await?;

            // Write trailing CRLF
            Self::write_to_stream(stream, CRLF.as_bytes()).await?;
        }

        // Write final chunk (size 0)
        let final_chunk = format!("0{}", CRLF);
        Self::write_to_stream(stream, final_chunk.as_bytes()).await?;

        // Write trailers
        for trailer in trailers {
            let trailer_line = format!("{}{}", trailer.to_string(), CRLF);
            Self::write_to_stream(stream, trailer_line.as_bytes()).await?;
        }

        // Final CRLF to end the message
        Self::write_to_stream(stream, CRLF.as_bytes()).await?;

        Ok(())
    }

    async fn read_response(
        stream: &mut TransportStream,
        method: &str,
    ) -> Result<Response, ProtocolError> {
        match stream {
            TransportStream::Tcp(tcp) => {
                let mut reader = BufReader::new(tcp);
                Self::read_response_from_reader(&mut reader, method).await
            }
            TransportStream::Tls(tls) => {
                let mut reader = BufReader::new(tls);
                Self::read_response_from_reader(&mut reader, method).await
            }
        }
    }

    async fn read_response_from_reader<R: AsyncBufRead + Unpin>(
        reader: &mut R,
        method: &str,
    ) -> Result<Response, ProtocolError> {
        loop {
            let mut status_line = String::new();
            let bytes = reader
                .read_line(&mut status_line)
                .await
                .map_err(ProtocolError::Io)?;

            if bytes == 0 {
                return Err(ProtocolError::InvalidResponse(
                    "Unexpected EOF while reading status line".to_string(),
                ));
            }

            if status_line.trim().is_empty() {
                // Skip empty lines between responses.
                continue;
            }

            let (status, protocol_version) = Self::parse_status_line(&status_line)?;
            let headers = Self::read_header_block(reader).await?;

            if status < 200 {
                // Informational response; skip body (per RFC 7230 §3.3.1) and continue.
                continue;
            }

            // Read body and trailers for the final response.
            let (body, trailers) = Self::read_body(reader, &headers, method, status).await?;

            return Ok(Response {
                status,
                protocol_version,
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

    pub fn parse_status_line(status_line: &str) -> Result<(u16, String), ProtocolError> {
        let parts: Vec<&str> = status_line.trim().split_whitespace().collect();
        if parts.len() < 2 {
            return Err(ProtocolError::InvalidResponse(
                "Invalid status line".to_string(),
            ));
        }

        let protocol_version = parts[0].to_string();
        let status_code = parts[1]
            .parse::<u16>()
            .map_err(|_| ProtocolError::InvalidResponse("Invalid status code".to_string()))?;

        Ok((status_code, protocol_version))
    }

    async fn read_body<R: AsyncBufRead + Unpin>(
        reader: &mut R,
        headers: &[Header],
        method: &str,
        status: u16,
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
            Self::read_chunked_body(reader).await
        } else {
            // Check for Content-Length
            let content_length = headers
                .iter()
                .find(|h| h.name.to_lowercase() == CONTENT_LENGTH_HEADER)
                .and_then(|h| h.value.as_ref())
                .and_then(|v| v.parse::<usize>().ok());

            if let Some(length) = content_length {
                let mut body = vec![0u8; length];
                reader
                    .read_exact(&mut body)
                    .await
                    .map_err(ProtocolError::Io)?;
                Ok((Bytes::from(body), Vec::new()))
            } else {
                // Read until connection close
                let mut body = Vec::new();
                reader
                    .read_to_end(&mut body)
                    .await
                    .map_err(ProtocolError::Io)?;
                Ok((Bytes::from(body), Vec::new()))
            }
        }
    }

    async fn read_chunked_body<R: AsyncBufRead + Unpin>(
        reader: &mut R,
    ) -> Result<(Bytes, Vec<Header>), ProtocolError> {
        let mut body = Vec::new();
        let mut trailers = Vec::new();

        loop {
            // Read chunk size line
            let mut size_line = String::new();
            reader
                .read_line(&mut size_line)
                .await
                .map_err(ProtocolError::Io)?;

            // Parse chunk size (hex)
            let size_str = size_line.trim().split(';').next().unwrap_or("").trim();
            let chunk_size = usize::from_str_radix(size_str, 16)
                .map_err(|_| ProtocolError::InvalidResponse("Invalid chunk size".to_string()))?;

            if chunk_size == 0 {
                // Read trailing headers
                loop {
                    let mut line = String::new();
                    reader
                        .read_line(&mut line)
                        .await
                        .map_err(ProtocolError::Io)?;

                    if line.trim().is_empty() {
                        break;
                    }

                    if let Some(trailer) = crate::utils::parse_header(line.trim()) {
                        trailers.push(trailer);
                    }
                }
                break;
            }

            // Read chunk data
            let mut chunk = vec![0u8; chunk_size];
            reader
                .read_exact(&mut chunk)
                .await
                .map_err(ProtocolError::Io)?;
            body.extend_from_slice(&chunk);

            // Read trailing CRLF
            let mut crlf = [0u8; 2];
            reader
                .read_exact(&mut crlf)
                .await
                .map_err(ProtocolError::Io)?;
        }

        Ok((Bytes::from(body), trailers))
    }

    async fn read_header_block<R: AsyncBufRead + Unpin>(
        reader: &mut R,
    ) -> Result<Vec<Header>, ProtocolError> {
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .await
                .map_err(ProtocolError::Io)?;

            if line.trim().is_empty() {
                break;
            }

            if let Some(header) = crate::utils::parse_header(line.trim()) {
                headers.push(header);
            }
        }
        Ok(headers)
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
}

#[async_trait(?Send)]
impl Protocol for H1Client {
    async fn send(&self, target: &Target, request: Request) -> Result<Response, ProtocolError> {
        self.send_request(target, request).await
    }
}
