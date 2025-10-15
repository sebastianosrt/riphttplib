use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring::default_provider;
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use rustls::DigitallySignedStruct;
use std::io;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time;
use tokio_rustls::{client::TlsStream, TlsConnector};

// pub const IO_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA1,
            rustls::SignatureScheme::ECDSA_SHA1_Legacy,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}

pub enum TransportStream {
    Tcp(TcpStream),
    Tls(TlsStream<TcpStream>),
}

async fn connect_tcp(host: &str, port: u16, timeout: Option<Duration>) -> io::Result<TcpStream> {
    if let Some(duration) = timeout {
        match time::timeout(duration, TcpStream::connect((host, port))).await {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "TCP connection timed out",
            )),
        }
    } else {
        TcpStream::connect((host, port)).await
    }
}

pub async fn create_tcp_stream(
    host: &str,
    port: u16,
    timeout: Option<Duration>,
) -> io::Result<TransportStream> {
    let stream = connect_tcp(host, port, timeout).await?;
    Ok(TransportStream::Tcp(stream))
}

pub async fn create_tls_stream(
    host: &str,
    port: u16,
    server_name: &str,
    timeout: Option<Duration>,
) -> io::Result<TransportStream> {
    // Ensure a crypto provider is installed (required for rustls >=0.23).
    let _ = default_provider().install_default();
    let tcp_stream = connect_tcp(host, port, timeout).await?;

    // Create TLS configuration that skips certificate verification
    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(NoCertificateVerification))
        .with_no_client_auth();

    // Enable HTTP/2 ALPN
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let connector = TlsConnector::from(std::sync::Arc::new(config));
    let server_name = ServerName::try_from(server_name.to_string())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid server name"))?;

    let tls_stream_fut = connector.connect(server_name, tcp_stream);
    let tls_stream = if let Some(duration) = timeout {
        match time::timeout(duration, tls_stream_fut).await {
            Ok(result) => result?,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "TLS handshake timed out",
                ))
            }
        }
    } else {
        tls_stream_fut.await?
    };
    Ok(TransportStream::Tls(tls_stream))
}

pub async fn create_h2_tls_stream(
    host: &str,
    port: u16,
    server_name: &str,
    timeout: Option<Duration>,
) -> io::Result<TransportStream> {
    let _ = default_provider().install_default();
    let tcp_stream = connect_tcp(host, port, timeout).await?;

    // Create TLS configuration that skips certificate verification
    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(NoCertificateVerification))
        .with_no_client_auth();

    // HTTP/2 ALPN only
    // TODO handle h2c
    config.alpn_protocols = vec![b"h2".to_vec()];

    let connector = TlsConnector::from(std::sync::Arc::new(config));
    let server_name = ServerName::try_from(server_name.to_string())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid server name"))?;

    let tls_stream_fut = connector.connect(server_name, tcp_stream);
    let tls_stream = if let Some(duration) = timeout {
        match time::timeout(duration, tls_stream_fut).await {
            Ok(result) => result?,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "TLS handshake timed out",
                ))
            }
        }
    } else {
        tls_stream_fut.await?
    };
    Ok(TransportStream::Tls(tls_stream))
}

pub async fn create_h2c_stream(
    host: &str,
    port: u16,
    timeout: Option<Duration>,
) -> io::Result<TransportStream> {
    let stream = connect_tcp(host, port, timeout).await?;
    Ok(TransportStream::Tcp(stream))
}

pub async fn create_stream(
    scheme: &str,
    host: &str,
    port: u16,
    timeout: Option<Duration>,
) -> io::Result<TransportStream> {
    match scheme {
        "http" => create_tcp_stream(host, port, timeout).await,
        "https" => create_tls_stream(host, port, host, timeout).await,
        "h2" => create_h2_tls_stream(host, port, host, timeout).await,
        "h2c" => create_h2c_stream(host, port, timeout).await,
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unsupported scheme: {}", scheme),
        )),
    }
}
