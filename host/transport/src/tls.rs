use std::{error::Error, fmt, sync::Arc};

use quinn::crypto::rustls::QuicServerConfig;
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::config::{CertificateSource, TransportClientConfig, TransportServerConfig};

#[derive(Debug, Clone)]
pub enum TlsConfigError {
    CertificateGeneration(String),
    RustlsConfig(String),
}

/// Generate a self-signed certificate for localhost development.
pub fn generate_self_signed_cert() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TlsConfigError> {
    let cert = generate_simple_self_signed(vec!["localhost".to_owned()])
        .map_err(|e| TlsConfigError::CertificateGeneration(e.to_string()))?;
    let key_der = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    Ok((vec![cert_der], PrivateKeyDer::Pkcs8(key_der)))
}

/// Build a quinn ServerConfig with the appropriate TLS settings.
pub fn build_server_config(
    config: &TransportServerConfig,
) -> Result<quinn::ServerConfig, TlsConfigError> {
    let (certs, key) = match &config.certificate {
        CertificateSource::SelfSigned => generate_self_signed_cert()?,
    };

    let mut rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsConfigError::RustlsConfig(e.to_string()))?;

    rustls_config.alpn_protocols = vec![config.alpn.as_bytes().to_vec()];

    let quic_config = QuicServerConfig::try_from(rustls_config)
        .map_err(|e| TlsConfigError::RustlsConfig(e.to_string()))?;

    Ok(quinn::ServerConfig::with_crypto(Arc::new(quic_config)))
}

/// Build a quinn ClientConfig with the appropriate TLS settings.
pub fn build_client_config(
    config: &TransportClientConfig,
) -> Result<quinn::ClientConfig, TlsConfigError> {
    let mut rustls_config = if config.debug_validation.allow_insecure_certificate_validation {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(InsecureCertVerifier))
            .with_no_client_auth()
    } else {
        rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth()
    };

    rustls_config.alpn_protocols = vec![config.alpn.as_bytes().to_vec()];

    Ok(quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(rustls_config)
            .map_err(|e| TlsConfigError::RustlsConfig(e.to_string()))?,
    )))
}

/// Certificate verifier that accepts any certificate (localhost development only).
#[derive(Debug)]
struct InsecureCertVerifier;

impl rustls::client::danger::ServerCertVerifier for InsecureCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

impl fmt::Display for TlsConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CertificateGeneration(error) => {
                write!(formatter, "certificate generation failed: {error}")
            }
            Self::RustlsConfig(error) => {
                write!(formatter, "TLS configuration failed: {error}")
            }
        }
    }
}

impl Error for TlsConfigError {}
