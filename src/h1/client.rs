use crate::stream::{create_stream, TransportStream};
use crate::types::{Header, Protocol, ProtocolError, Request, Response, Target};
use crate::utils::{
    CHUNKED_ENCODING, CONTENT_LENGTH_HEADER, CRLF, HOST_HEADER, HTTP_VERSION_1_1,
    TRANSFER_ENCODING_HEADER, USER_AGENT, USER_AGENT_HEADER,
};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub struct H1Client;

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

    async fn write_request(
        stream: &mut TransportStream,
        target: &Target,
        request: &Request,
    ) -> Result<(), ProtocolError> {
        let mut req = Vec::new();
        let path = target.path();

        let empty_trailers = Vec::new();
        let mut headers = request.headers.clone();
        let trailers = request.trailers.as_ref().unwrap_or(&empty_trailers);

        req.extend_from_slice(
            format!("{} {} {}{}", request.method, path, HTTP_VERSION, CRLF).as_bytes(),
        ); // request line

        let has_host = headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(HOST_HEADER));
        if !has_host {
            let authority = target
                .authority()
                .unwrap_or_else(|| target.host().unwrap_or_default().to_string());
            headers.push(Header::new(HOST_HEADER.to_string(), authority.to_string()));
        }

        let has_user_agent = headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(USER_AGENT_HEADER));
        if !has_user_agent {
            headers.push(Header::new(
                USER_AGENT_HEADER.to_string(),
                USER_AGENT.to_string(),
            ));
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

        // End headers
        req.extend_from_slice(CRLF.as_bytes());

        // Write body and trailers
        let empty_body = Bytes::new();
        if use_chunked {
            let body = request.body.as_ref().unwrap_or(&empty_body);
            let chunked_body = Self::build_chunked_body(body, trailers.as_slice());
            req.extend_from_slice(&chunked_body);
        } else if let Some(body) = request.body.as_ref() {
            req.extend_from_slice(body);
        }

        Self::write_to_stream(stream, &req).await?;

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

    fn build_chunked_body(body: &Bytes, trailers: &[Header]) -> Vec<u8> {
        let mut chunked_body = Vec::new();

        if !body.is_empty() {
            // Write chunk size in hex
            let chunk_size = format!("{:x}{}", body.len(), CRLF);
            chunked_body.extend_from_slice(chunk_size.as_bytes());

            // Write chunk data
            chunked_body.extend_from_slice(body);

            // Write trailing CRLF
            chunked_body.extend_from_slice(CRLF.as_bytes());
        }

        // Write final chunk (size 0)
        let final_chunk = format!("0{}", CRLF);
        chunked_body.extend_from_slice(final_chunk.as_bytes());

        // Write trailers
        for trailer in trailers {
            let trailer_line = format!("{}{}", trailer.to_string(), CRLF);
            chunked_body.extend_from_slice(trailer_line.as_bytes());
        }

        // Final CRLF to end the message
        chunked_body.extend_from_slice(CRLF.as_bytes());

        chunked_body
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
