use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring::default_provider;
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use rustls::DigitallySignedStruct;
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time;
use tokio_rustls::{client::TlsStream, TlsConnector};

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

const ALPN_HTTP11: &[u8] = b"http/1.1";
const ALPN_H2: &[u8] = b"h2";

fn build_alpn_list(protocols: Option<&[&[u8]]>) -> Vec<Vec<u8>> {
    match protocols {
        Some(list) if !list.is_empty() => list.iter().map(|p| p.to_vec()).collect(),
        _ => vec![ALPN_HTTP11.to_vec()],
    }
}

fn server_name_from_str(name: &str) -> io::Result<ServerName<'static>> {
    ServerName::try_from(name.to_string()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid server name: {}", name),
        )
    })
}

fn build_tls_connector(protocols: Option<&[&[u8]]>) -> TlsConnector {
    let mut config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
        .with_no_client_auth();

    config.alpn_protocols = build_alpn_list(protocols);

    TlsConnector::from(Arc::new(config))
}

async fn with_timeout<F, T>(
    duration: Option<Duration>,
    future: F,
    timeout_message: &'static str,
) -> io::Result<T>
where
    F: Future<Output = io::Result<T>>,
{
    if let Some(duration) = duration {
        match time::timeout(duration, future).await {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, timeout_message)),
        }
    } else {
        future.await
    }
}

async fn connect_tcp(host: &str, port: u16, timeout: Option<Duration>) -> io::Result<TcpStream> {
    let connect_future = TcpStream::connect((host, port));
    with_timeout(timeout, connect_future, "TCP connection timed out").await
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
    timeout: Option<Duration>,
    alpn_protocols: Option<&[&[u8]]>,
) -> io::Result<TransportStream> {
    // Ensure a crypto provider is installed (required for rustls >=0.23).
    let _ = default_provider().install_default();
    let tcp_stream = connect_tcp(host, port, timeout).await?;

    let connector = build_tls_connector(alpn_protocols);
    let server_name = server_name_from_str(host)?;

    let tls_stream = with_timeout(
        timeout,
        connector.connect(server_name, tcp_stream),
        "TLS handshake timed out",
    )
    .await?;

    Ok(TransportStream::Tls(tls_stream))
}

pub async fn create_stream(
    scheme: &str,
    host: &str,
    port: u16,
    timeout: Option<Duration>,
) -> io::Result<TransportStream> {
    match scheme {
        "http" => create_tcp_stream(host, port, timeout).await,
        "https" => create_tls_stream(host, port, timeout, Some(&[ALPN_HTTP11])).await,
        "h2" => create_tls_stream(host, port, timeout, Some(&[ALPN_H2])).await,
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unsupported scheme: {}", scheme),
        )),
    }
}
