use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig as QuinnClientConfig, Connection, Endpoint};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::ServerName;
use rustls::DigitallySignedStruct;
use rustls::ClientConfig;
use std::io;
use std::sync::Arc;
use tokio::net::lookup_host;
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};

// pub const IO_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct NoCertificateVerification;

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

pub async fn create_tcp_stream(host: &str, port: u16) -> io::Result<TransportStream> {
    let stream = TcpStream::connect((host, port)).await?;
    Ok(TransportStream::Tcp(stream))
}

pub async fn create_tls_stream(host: &str, port: u16, server_name: &str) -> io::Result<TransportStream> {
    let tcp_stream = TcpStream::connect((host, port)).await?;

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

    let tls_stream = connector.connect(server_name, tcp_stream).await?;
    Ok(TransportStream::Tls(tls_stream))
}

pub async fn create_quic_connection(
    host: &str,
    port: u16,
    server_name: &str,
) -> io::Result<Connection> {
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;

    let mut rustls_config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
        .with_no_client_auth();
    rustls_config.alpn_protocols = vec![b"h3".to_vec()];

    let quic_crypto = QuicClientConfig::try_from(rustls_config)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let client_config = QuinnClientConfig::new(Arc::new(quic_crypto));
    endpoint.set_default_client_config(client_config);

    // Resolve hostname to addresses (DNS) and pick the first
    let mut addrs = lookup_host((host, port)).await.map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("DNS lookup failed for {}:{}: {}", host, port, e),
        )
    })?;
    let addr = addrs.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("No addresses found for {}:{}", host, port),
        )
    })?;

    let connecting = endpoint
        .connect(addr, server_name)
        .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;
    connecting
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))
}

pub async fn create_h2_tls_stream(
    host: &str,
    port: u16,
    server_name: &str,
) -> io::Result<TransportStream> {
    let tcp_stream = TcpStream::connect((host, port)).await?;

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

    let tls_stream = connector.connect(server_name, tcp_stream).await?;
    Ok(TransportStream::Tls(tls_stream))
}

// TODO handle h2c
pub async fn create_stream(scheme: &str, host: &str, port: u16) -> io::Result<TransportStream> {
    match scheme {
        "http" => create_tcp_stream(host, port).await,
        "https" => create_tls_stream(host, port, host).await,
        "h2" => create_h2_tls_stream(host, port, host).await,
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unsupported scheme: {}", scheme),
        )),
    }
}
