use std::{error::Error, fmt};

use msquic::{
    CertificateHash, CertificateHashStore, CertificateHashStoreFlags, Credential,
    CredentialConfig, CredentialFlags,
};

use crate::config::{CertificateSource, TransportClientConfig, TransportServerConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsConfigError {
    EmptyCertificateThumbprint,
    EmptyCertificateStore,
    InvalidCertificateThumbprint { actual: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsQuicCredentialBinding {
    certificate_summary: String,
    validation_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientValidationBinding {
    mode: &'static str,
    detail: String,
}

pub fn build_server_credential_config(
    config: &TransportServerConfig,
) -> Result<CredentialConfig, TlsConfigError> {
    match &config.certificate {
        CertificateSource::WindowsCertificateHash {
            sha1_thumbprint,
            store_name,
            use_machine_store,
        } => {
            let normalized_thumbprint = normalized_thumbprint(sha1_thumbprint)?;
            let store_name = normalized_store_name(store_name)?;
            let certificate_hash = CertificateHash::from_str(&normalized_thumbprint)
                .map_err(|_| TlsConfigError::InvalidCertificateThumbprint {
                    actual: normalized_thumbprint.len(),
                })?;

            let mut store_flags = CertificateHashStoreFlags::NONE;
            if *use_machine_store {
                store_flags.insert(CertificateHashStoreFlags::MACHINE_STORE);
            }

            Ok(CredentialConfig::new().set_credential(Credential::CertificateHashStore(
                CertificateHashStore::new(
                    store_flags,
                    certificate_hash.to_hex_string().as_bytes().chunks_exact(2).map(|chunk| {
                        u8::from_str_radix(std::str::from_utf8(chunk).expect("thumbprint chunk"), 16)
                            .expect("validated thumbprint chunk")
                    }).collect::<Vec<u8>>().try_into().expect("validated SHA-1 thumbprint"),
                    store_name,
                ),
            )))
        }
    }
}

pub fn build_client_credential_config(config: &TransportClientConfig) -> CredentialConfig {
    let mut credential = CredentialConfig::new_client();
    if config.debug_validation.allow_insecure_certificate_validation {
        credential = credential.set_credential_flags(CredentialFlags::NO_CERTIFICATE_VALIDATION);
    }
    credential
}

impl MsQuicCredentialBinding {
    pub fn from_server_config(config: &TransportServerConfig) -> Result<Self, TlsConfigError> {
        let certificate_summary = match &config.certificate {
            CertificateSource::WindowsCertificateHash {
                sha1_thumbprint,
                store_name,
                use_machine_store,
            } => {
                let normalized_thumbprint = normalized_thumbprint(sha1_thumbprint)?;
                let store_name = normalized_store_name(store_name)?;

                format!(
                    "cert-hash-sha1={} store={} location={}",
                    normalized_thumbprint,
                    store_name,
                    if *use_machine_store {
                        "LocalMachine"
                    } else {
                        "CurrentUser"
                    }
                )
            }
        };

        let validation_summary = if config.debug_validation.allow_insecure_certificate_validation {
            "debug-insecure-allowed".to_owned()
        } else {
            "server-certificate-required".to_owned()
        };

        Ok(Self {
            certificate_summary,
            validation_summary,
        })
    }

    pub fn describe(&self) -> String {
        format!("{} ({})", self.certificate_summary, self.validation_summary)
    }
}

impl ClientValidationBinding {
    pub fn from_client_config(config: &TransportClientConfig) -> Self {
        if config.debug_validation.allow_insecure_certificate_validation {
            return Self {
                mode: "debug-insecure",
                detail: "certificate verification bypassed for local development".to_owned(),
            };
        }

        Self {
            mode: "system-trust",
            detail: "use OS trust store for dev certificate".to_owned(),
        }
    }

    pub fn describe(&self) -> String {
        format!("{} ({})", self.mode, self.detail)
    }
}

fn normalize_hex(value: &str) -> String {
    value.replace(':', "").trim().to_ascii_lowercase()
}

fn normalized_thumbprint(value: &str) -> Result<String, TlsConfigError> {
    let normalized_thumbprint = normalize_hex(value);
    if normalized_thumbprint.is_empty() {
        return Err(TlsConfigError::EmptyCertificateThumbprint);
    }
    if normalized_thumbprint.len() != 40 {
        return Err(TlsConfigError::InvalidCertificateThumbprint {
            actual: normalized_thumbprint.len(),
        });
    }

    Ok(normalized_thumbprint)
}

fn normalized_store_name(value: &str) -> Result<String, TlsConfigError> {
    let store_name = value.trim();
    if store_name.is_empty() {
        return Err(TlsConfigError::EmptyCertificateStore);
    }

    Ok(store_name.to_owned())
}

impl fmt::Display for TlsConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCertificateThumbprint => {
                write!(formatter, "Windows certificate SHA-1 thumbprint must not be empty")
            }
            Self::EmptyCertificateStore => {
                write!(formatter, "Windows certificate store name must not be empty")
            }
            Self::InvalidCertificateThumbprint { actual } => {
                write!(formatter, "Windows certificate SHA-1 thumbprint must contain 40 hex characters, got {actual}")
            }
        }
    }
}

impl Error for TlsConfigError {}