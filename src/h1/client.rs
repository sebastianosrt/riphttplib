use crate::stream::{StreamType, create_stream};
use crate::types::{Header, Protocol, ProtocolError, Response, Target};
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

    async fn write_request(
        stream: &mut StreamType,
        method: &str,
        target: &Target,
        headers: Option<&Vec<Header>>,
        body: Option<&Bytes>,
        trailers: Option<&Vec<Header>>,
    ) -> Result<(), ProtocolError> {
        let path = &target.path;

        let request_line = format!("{} {} {}{}", method, path, HTTP_VERSION, CRLF);
        Self::write_to_stream(stream, request_line.as_bytes()).await?;

        let empty_headers = Vec::new();
        let empty_trailers = Vec::new();
        let headers = headers.unwrap_or(&empty_headers);
        let trailers = trailers.unwrap_or(&empty_trailers);

        let has_host = headers.iter().any(|h| h.name.to_lowercase() == HOST_HEADER);
        if !has_host {
            let host_header = format!("Host: {}{}", target.host, CRLF);
            Self::write_to_stream(stream, host_header.as_bytes()).await?;
        }

        let has_user_agent = headers
            .iter()
            .any(|h| h.name.to_lowercase() == USER_AGENT_HEADER);
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
            .any(|h| h.name.to_lowercase() == CONTENT_LENGTH_HEADER);
        let has_chunked = headers.iter().any(|h| {
            h.name.to_lowercase() == TRANSFER_ENCODING_HEADER
                && h.value
                    .as_ref()
                    .map_or(false, |v| v.to_lowercase().contains(CHUNKED_ENCODING))
        });

        let use_chunked = !trailers.is_empty() || !has_content_length || has_chunked;

        if let Some(body) = body {
            if use_chunked {
                // Use chunked encoding if we have trailers or no explicit Content-Length
                let chunked_header = format!("Transfer-Encoding: {}{}", CHUNKED_ENCODING, CRLF);
                Self::write_to_stream(stream, chunked_header.as_bytes()).await?;
            } else {
                // Add Content-Length if not chunked and not already present
                let content_length = format!("Content-Length: {}{}", body.len(), CRLF);
                Self::write_to_stream(stream, content_length.as_bytes()).await?;
            }
        }

        // End headers
        Self::write_to_stream(stream, CRLF.as_bytes()).await?;

        // Write body
        if let Some(body) = body {
            if use_chunked {
                // Write as chunked
                Self::write_chunked_body(stream, body, trailers).await?;
            } else {
                // Write as regular body
                Self::write_to_stream(stream, body).await?;
            }
        } else if use_chunked {
            Self::write_chunked_body(stream, &Bytes::new(), trailers).await?;
        }

        Ok(())
    }

    async fn write_to_stream(stream: &mut StreamType, data: &[u8]) -> Result<(), ProtocolError> {
        match stream {
            StreamType::Tcp(tcp) => tcp.write_all(data).await.map_err(ProtocolError::Io)?,
            StreamType::Tls(tls) => tls.write_all(data).await.map_err(ProtocolError::Io)?,
            StreamType::Quic(_) => {
                return Err(ProtocolError::RequestFailed(
                    "QUIC not supported for HTTP/1.1".to_string(),
                ));
            }
        }
        Ok(())
    }

    async fn write_chunked_body(
        stream: &mut StreamType,
        body: &Bytes,
        trailers: &Vec<Header>,
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
        stream: &mut StreamType,
        method: &str,
    ) -> Result<Response, ProtocolError> {
        match stream {
            StreamType::Tcp(tcp) => {
                let mut reader = BufReader::new(tcp);
                Self::read_response_from_reader(&mut reader, method).await
            }
            StreamType::Tls(tls) => {
                let mut reader = BufReader::new(tls);
                Self::read_response_from_reader(&mut reader, method).await
            }
            StreamType::Quic(_) => Err(ProtocolError::RequestFailed(
                "QUIC not supported for HTTP/1.1".to_string(),
            )),
        }
    }

    async fn read_response_from_reader<R: AsyncBufRead + Unpin>(
        reader: &mut R,
        method: &str,
    ) -> Result<Response, ProtocolError> {
        // Read status line
        let mut status_line = String::new();
        reader
            .read_line(&mut status_line)
            .await
            .map_err(ProtocolError::Io)?;

        let (status, protocol_version) = Self::parse_status_line(&status_line)?;

        // Read headers
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

        // Read body and trailers
        let (body, trailers) = Self::read_body(reader, &headers, method).await?;

        Ok(Response {
            status,
            protocol_version,
            headers,
            body,
            trailers: if trailers.is_empty() {
                None
            } else {
                Some(trailers)
            },
        })
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
    ) -> Result<(Bytes, Vec<Header>), ProtocolError> {
        // HEAD responses should not have a body according to RFC 7231
        if method.to_uppercase() == "HEAD" {
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
                // Read trailing headers (trailers)
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
}

#[async_trait]
impl Protocol for H1Client {
    async fn send(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(Bytes::from);
        self.send_bytes(target, method, headers, body_bytes, trailers)
            .await
    }

    async fn get(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        self.send(target, Some("GET".to_string()), headers, body, trailers)
            .await
    }

    async fn post(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        self.send(target, Some("POST".to_string()), headers, body, trailers)
            .await
    }

    async fn send_bytes(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let method = method.unwrap_or_else(|| "GET".to_string());

        let mut stream = create_stream(&target.scheme, &target.host, target.port)
            .await
            .map_err(|e| ProtocolError::ConnectionFailed(e.to_string()))?;

        Self::write_request(
            &mut stream,
            &method,
            target,
            headers.as_ref(),
            body.as_ref(),
            trailers.as_ref(),
        )
        .await?;
        Self::read_response(&mut stream, &method).await
    }

    async fn get_bytes(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        self.send_bytes(target, Some("GET".to_string()), headers, body, trailers)
            .await
    }

    async fn post_bytes(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<Bytes>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        self.send_bytes(target, Some("POST".to_string()), headers, body, trailers)
            .await
    }
}

impl H1Client {
    pub async fn send_str(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<&str>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(|s| Bytes::from(s.to_string()));
        self.send_bytes(target, method, headers, body_bytes, trailers)
            .await
    }

    pub async fn get_str(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<&str>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(|s| Bytes::from(s.to_string()));
        self.get_bytes(target, headers, body_bytes, trailers).await
    }

    pub async fn post_str(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<&str>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(|s| Bytes::from(s.to_string()));
        self.post_bytes(target, headers, body_bytes, trailers).await
    }

    pub async fn send_string(
        &self,
        target: &Target,
        method: Option<String>,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(Bytes::from);
        self.send_bytes(target, method, headers, body_bytes, trailers)
            .await
    }

    pub async fn get_string(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(Bytes::from);
        self.get_bytes(target, headers, body_bytes, trailers).await
    }

    pub async fn post_string(
        &self,
        target: &Target,
        headers: Option<Vec<Header>>,
        body: Option<String>,
        trailers: Option<Vec<Header>>,
    ) -> Result<Response, ProtocolError> {
        let body_bytes = body.map(Bytes::from);
        self.post_bytes(target, headers, body_bytes, trailers).await
    }
}
