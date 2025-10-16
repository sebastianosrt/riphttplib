use crate::stream::TransportStream;
use crate::types::{ProxyConfig, ProxyType, ProtocolError};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use rustls::{ClientConfig, RootCertStore};
use rustls::pki_types::ServerName;
use std::sync::Arc;
use webpki_roots;

/// Establishes a connection through a proxy
pub async fn connect_through_proxy(
    proxy: &ProxyConfig,
    target_host: &str,
    target_port: u16,
    connect_timeout: Option<Duration>,
) -> Result<TransportStream, ProtocolError> {
    let proxy_host = proxy.url.host_str()
        .ok_or_else(|| ProtocolError::InvalidTarget("Proxy missing host".to_string()))?;
    let proxy_port = proxy.url.port_or_known_default()
        .ok_or_else(|| ProtocolError::InvalidTarget("Proxy missing port".to_string()))?;

    match proxy.proxy_type {
        ProxyType::Http | ProxyType::Https => {
            connect_http_proxy(proxy_host, proxy_port, target_host, target_port, proxy, connect_timeout).await
        }
        ProxyType::Socks5 => {
            connect_socks5_proxy(proxy_host, proxy_port, target_host, target_port, proxy, connect_timeout).await
        }
        ProxyType::Socks4 => {
            connect_socks4_proxy(proxy_host, proxy_port, target_host, target_port, proxy, connect_timeout).await
        }
    }
}

/// Establishes a connection through a proxy for HTTPS target
pub async fn connect_through_proxy_https(
    proxy: &ProxyConfig,
    target_host: &str,
    target_port: u16,
    connect_timeout: Option<Duration>,
) -> Result<TransportStream, ProtocolError> {
    // First establish the proxy connection
    let stream = connect_through_proxy(proxy, target_host, target_port, connect_timeout).await?;

    // For HTTP proxies, we need to upgrade to TLS after CONNECT
    match proxy.proxy_type {
        ProxyType::Http | ProxyType::Https => {
            // The stream is already tunneled through CONNECT, now upgrade to TLS
            if let TransportStream::Tcp(tcp_stream) = stream {
                upgrade_to_tls(tcp_stream, target_host).await
            } else {
                Ok(stream) // Already TLS
            }
        }
        ProxyType::Socks5 | ProxyType::Socks4 => {
            // For SOCKS proxies, we need to upgrade the tunneled connection to TLS
            if let TransportStream::Tcp(tcp_stream) = stream {
                upgrade_to_tls(tcp_stream, target_host).await
            } else {
                Ok(stream)
            }
        }
    }
}

/// Upgrades a TCP stream to TLS
async fn upgrade_to_tls(tcp_stream: TcpStream, target_host: &str) -> Result<TransportStream, ProtocolError> {
    // Install default crypto provider if not already installed
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    // Enable SNI
    let connector = TlsConnector::from(Arc::new(config));
    let domain = ServerName::try_from(target_host.to_string())
        .map_err(|e| ProtocolError::ConnectionFailed(format!("Invalid domain for TLS: {}", e)))?;

    let tls_stream = connector.connect(domain, tcp_stream).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("TLS handshake failed: {}", e)))?;

    Ok(TransportStream::Tls(tls_stream))
}

/// Connects through HTTP/HTTPS proxy using HTTP CONNECT method
async fn connect_http_proxy(
    proxy_host: &str,
    proxy_port: u16,
    target_host: &str,
    target_port: u16,
    proxy: &ProxyConfig,
    connect_timeout: Option<Duration>,
) -> Result<TransportStream, ProtocolError> {
    // Connect to proxy
    let mut stream = connect_to_proxy_tcp(proxy_host, proxy_port, connect_timeout).await?;

    // Send CONNECT request
    let connect_request = format!(
        "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n",
        target_host, target_port, target_host, target_port
    );

    let mut request_lines = vec![connect_request];

    // Add proxy authentication if provided
    if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
        let auth_str = format!("{}:{}", username, password);
        let auth_encoded = base64_encode(auth_str.as_bytes());
        request_lines.push(format!("Proxy-Authorization: Basic {}\r\n", auth_encoded));
    }

    request_lines.push("\r\n".to_string());
    let full_request = request_lines.join("");

    // Send CONNECT request
    stream.write_all(full_request.as_bytes()).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("Failed to send CONNECT: {}", e)))?;

    // Read response
    let mut buffer = vec![0; 1024];
    let n = stream.read(&mut buffer).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("Failed to read CONNECT response: {}", e)))?;

    let response = String::from_utf8_lossy(&buffer[..n]);

    // Check if CONNECT was successful (200 status code)
    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        return Err(ProtocolError::ConnectionFailed(
            format!("Proxy CONNECT failed: {}", response.lines().next().unwrap_or("Unknown error"))
        ));
    }

    Ok(TransportStream::Tcp(stream))
}

/// Connects through SOCKS5 proxy
async fn connect_socks5_proxy(
    proxy_host: &str,
    proxy_port: u16,
    target_host: &str,
    target_port: u16,
    proxy: &ProxyConfig,
    connect_timeout: Option<Duration>,
) -> Result<TransportStream, ProtocolError> {
    let mut stream = connect_to_proxy_tcp(proxy_host, proxy_port, connect_timeout).await?;

    // SOCKS5 greeting
    let has_auth = proxy.username.is_some() && proxy.password.is_some();
    let auth_methods = if has_auth { vec![0x00, 0x02] } else { vec![0x00] }; // No auth, Username/Password

    let mut greeting = vec![0x05, auth_methods.len() as u8];
    greeting.extend_from_slice(&auth_methods);

    stream.write_all(&greeting).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 greeting failed: {}", e)))?;

    // Read greeting response
    let mut response = vec![0; 2];
    stream.read_exact(&mut response).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 greeting response failed: {}", e)))?;

    if response[0] != 0x05 {
        return Err(ProtocolError::ConnectionFailed("Invalid SOCKS5 response".to_string()));
    }

    // Handle authentication
    match response[1] {
        0x00 => {}, // No authentication required
        0x02 => {
            // Username/password authentication
            if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
                socks5_authenticate(&mut stream, username, password).await?;
            } else {
                return Err(ProtocolError::ConnectionFailed("SOCKS5 authentication required but no credentials provided".to_string()));
            }
        }
        0xFF => {
            return Err(ProtocolError::ConnectionFailed("SOCKS5 no acceptable authentication methods".to_string()));
        }
        _ => {
            return Err(ProtocolError::ConnectionFailed("SOCKS5 unsupported authentication method".to_string()));
        }
    }

    // Send connect request
    socks5_connect(&mut stream, target_host, target_port).await?;

    Ok(TransportStream::Tcp(stream))
}

/// SOCKS5 username/password authentication
async fn socks5_authenticate(
    stream: &mut TcpStream,
    username: &str,
    password: &str,
) -> Result<(), ProtocolError> {
    let mut auth_request = vec![0x01]; // Version
    auth_request.push(username.len() as u8);
    auth_request.extend_from_slice(username.as_bytes());
    auth_request.push(password.len() as u8);
    auth_request.extend_from_slice(password.as_bytes());

    stream.write_all(&auth_request).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 auth request failed: {}", e)))?;

    let mut response = vec![0; 2];
    stream.read_exact(&mut response).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 auth response failed: {}", e)))?;

    if response[1] != 0x00 {
        return Err(ProtocolError::ConnectionFailed("SOCKS5 authentication failed".to_string()));
    }

    Ok(())
}

/// SOCKS5 connect request
async fn socks5_connect(
    stream: &mut TcpStream,
    target_host: &str,
    target_port: u16,
) -> Result<(), ProtocolError> {
    let mut connect_request = vec![0x05, 0x01, 0x00]; // Version, Connect, Reserved

    // Address type and address
    if target_host.parse::<std::net::IpAddr>().is_ok() {
        // IPv4/IPv6 address
        if let Ok(ipv4) = target_host.parse::<std::net::Ipv4Addr>() {
            connect_request.push(0x01); // IPv4
            connect_request.extend_from_slice(&ipv4.octets());
        } else if let Ok(ipv6) = target_host.parse::<std::net::Ipv6Addr>() {
            connect_request.push(0x04); // IPv6
            connect_request.extend_from_slice(&ipv6.octets());
        }
    } else {
        // Domain name
        connect_request.push(0x03); // Domain name
        connect_request.push(target_host.len() as u8);
        connect_request.extend_from_slice(target_host.as_bytes());
    }

    // Port
    connect_request.extend_from_slice(&target_port.to_be_bytes());

    stream.write_all(&connect_request).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 connect request failed: {}", e)))?;

    // Read connect response
    let mut response = vec![0; 4];
    stream.read_exact(&mut response).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 connect response failed: {}", e)))?;

    if response[0] != 0x05 || response[1] != 0x00 {
        let error_msg = match response[1] {
            0x01 => "General SOCKS server failure",
            0x02 => "Connection not allowed by ruleset",
            0x03 => "Network unreachable",
            0x04 => "Host unreachable",
            0x05 => "Connection refused",
            0x06 => "TTL expired",
            0x07 => "Command not supported",
            0x08 => "Address type not supported",
            _ => "Unknown SOCKS5 error",
        };
        return Err(ProtocolError::ConnectionFailed(format!("SOCKS5 connect failed: {}", error_msg)));
    }

    // Read the rest of the response (address and port)
    match response[3] {
        0x01 => {
            // IPv4
            let mut addr_port = vec![0; 6]; // 4 bytes IP + 2 bytes port
            stream.read_exact(&mut addr_port).await
                .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 IPv4 response read failed: {}", e)))?;
        }
        0x03 => {
            // Domain name
            let mut len_buf = vec![0; 1];
            stream.read_exact(&mut len_buf).await
                .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 domain length read failed: {}", e)))?;
            let domain_len = len_buf[0] as usize;
            let mut domain_port = vec![0; domain_len + 2]; // domain + 2 bytes port
            stream.read_exact(&mut domain_port).await
                .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 domain response read failed: {}", e)))?;
        }
        0x04 => {
            // IPv6
            let mut addr_port = vec![0; 18]; // 16 bytes IP + 2 bytes port
            stream.read_exact(&mut addr_port).await
                .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS5 IPv6 response read failed: {}", e)))?;
        }
        _ => {
            return Err(ProtocolError::ConnectionFailed("SOCKS5 unsupported address type in response".to_string()));
        }
    }

    Ok(())
}

/// Connects through SOCKS4 proxy
async fn connect_socks4_proxy(
    proxy_host: &str,
    proxy_port: u16,
    target_host: &str,
    target_port: u16,
    proxy: &ProxyConfig,
    connect_timeout: Option<Duration>,
) -> Result<TransportStream, ProtocolError> {
    let mut stream = connect_to_proxy_tcp(proxy_host, proxy_port, connect_timeout).await?;

    // Resolve target host to IP (SOCKS4 requires IP address)
    let target_ip = match target_host.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => ip,
        Err(_) => {
            // Try to resolve hostname
            match tokio::net::lookup_host(format!("{}:{}", target_host, target_port)).await {
                Ok(mut addrs) => {
                    if let Some(addr) = addrs.next() {
                        if let std::net::IpAddr::V4(ipv4) = addr.ip() {
                            ipv4
                        } else {
                            return Err(ProtocolError::ConnectionFailed("SOCKS4 only supports IPv4".to_string()));
                        }
                    } else {
                        return Err(ProtocolError::ConnectionFailed("Failed to resolve hostname for SOCKS4".to_string()));
                    }
                }
                Err(e) => {
                    return Err(ProtocolError::ConnectionFailed(format!("Failed to resolve hostname: {}", e)));
                }
            }
        }
    };

    // Build SOCKS4 connect request
    let mut connect_request = vec![0x04, 0x01]; // Version, Connect command
    connect_request.extend_from_slice(&target_port.to_be_bytes()); // Port
    connect_request.extend_from_slice(&target_ip.octets()); // IP address

    // User ID (can be empty)
    if let Some(username) = &proxy.username {
        connect_request.extend_from_slice(username.as_bytes());
    }
    connect_request.push(0x00); // Null terminator

    stream.write_all(&connect_request).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS4 connect request failed: {}", e)))?;

    // Read response
    let mut response = vec![0; 8];
    stream.read_exact(&mut response).await
        .map_err(|e| ProtocolError::ConnectionFailed(format!("SOCKS4 connect response failed: {}", e)))?;

    if response[0] != 0x00 || response[1] != 0x5a {
        let error_msg = match response[1] {
            0x5b => "Request rejected or failed",
            0x5c => "Request rejected because SOCKS server cannot connect to identd on the client",
            0x5d => "Request rejected because the client program and identd report different user-ids",
            _ => "Unknown SOCKS4 error",
        };
        return Err(ProtocolError::ConnectionFailed(format!("SOCKS4 connect failed: {}", error_msg)));
    }

    Ok(TransportStream::Tcp(stream))
}

/// Connect to proxy TCP socket with timeout
async fn connect_to_proxy_tcp(
    proxy_host: &str,
    proxy_port: u16,
    connect_timeout: Option<Duration>,
) -> Result<TcpStream, ProtocolError> {
    let connect_future = TcpStream::connect((proxy_host, proxy_port));

    let stream = if let Some(timeout_duration) = connect_timeout {
        timeout(timeout_duration, connect_future)
            .await
            .map_err(|_| ProtocolError::Timeout)?
            .map_err(|e| ProtocolError::ConnectionFailed(format!("Failed to connect to proxy: {}", e)))?
    } else {
        connect_future
            .await
            .map_err(|e| ProtocolError::ConnectionFailed(format!("Failed to connect to proxy: {}", e)))?
    };

    Ok(stream)
}

/// Simple base64 encoding for HTTP proxy authentication
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    for chunk in input.chunks(3) {
        let mut buf = [0u8; 3];
        for (i, &b) in chunk.iter().enumerate() {
            buf[i] = b;
        }

        let b0 = buf[0] as usize;
        let b1 = buf[1] as usize;
        let b2 = buf[2] as usize;

        result.push(CHARS[b0 >> 2] as char);
        result.push(CHARS[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if chunk.len() > 1 {
            result.push(CHARS[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARS[b2 & 0x3f] as char);
        } else {
            result.push('=');
        }
    }

    result
}