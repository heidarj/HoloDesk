use std::env;

use crate::protocol::DEFAULT_ALPN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertificateSource {
    SelfSigned,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DebugTlsSettings {
    pub allow_insecure_certificate_validation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub alpn: String,
    pub certificate: CertificateSource,
    pub debug_validation: DebugTlsSettings,
    pub server_initiated_close_after_ack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportClientConfig {
    pub server_host: String,
    pub server_port: u16,
    pub server_name: Option<String>,
    pub alpn: String,
    pub debug_validation: DebugTlsSettings,
    pub send_goodbye_after_ack: bool,
}

impl Default for CertificateSource {
    fn default() -> Self {
        Self::SelfSigned
    }
}

impl Default for TransportServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_owned(),
            port: 4433,
            alpn: DEFAULT_ALPN.to_owned(),
            certificate: CertificateSource::default(),
            debug_validation: DebugTlsSettings::default(),
            server_initiated_close_after_ack: false,
        }
    }
}

impl Default for TransportClientConfig {
    fn default() -> Self {
        Self {
            server_host: "127.0.0.1".to_owned(),
            server_port: 4433,
            server_name: Some("localhost".to_owned()),
            alpn: DEFAULT_ALPN.to_owned(),
            debug_validation: DebugTlsSettings::default(),
            send_goodbye_after_ack: true,
        }
    }
}

impl TransportServerConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            bind_address: env::var("HOLOBRIDGE_TRANSPORT_BIND")
                .unwrap_or_else(|_| defaults.bind_address.clone()),
            port: env_u16("HOLOBRIDGE_TRANSPORT_PORT").unwrap_or(defaults.port),
            alpn: env::var("HOLOBRIDGE_TRANSPORT_ALPN").unwrap_or_else(|_| defaults.alpn.clone()),
            certificate: CertificateSource::SelfSigned,
            debug_validation: DebugTlsSettings::from_env(),
            server_initiated_close_after_ack: env_bool("HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK")
                .unwrap_or(defaults.server_initiated_close_after_ack),
        }
    }

    pub fn listen_endpoint(&self) -> String {
        format!("{}:{}", self.bind_address, self.port)
    }
}

impl TransportClientConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            server_host: env::var("HOLOBRIDGE_TRANSPORT_HOST")
                .unwrap_or_else(|_| defaults.server_host.clone()),
            server_port: env_u16("HOLOBRIDGE_TRANSPORT_PORT").unwrap_or(defaults.server_port),
            server_name: env::var("HOLOBRIDGE_TRANSPORT_SERVER_NAME")
                .ok()
                .or(defaults.server_name),
            alpn: env::var("HOLOBRIDGE_TRANSPORT_ALPN").unwrap_or_else(|_| defaults.alpn.clone()),
            debug_validation: DebugTlsSettings::from_env(),
            send_goodbye_after_ack: env_bool("HOLOBRIDGE_TRANSPORT_CLIENT_SEND_GOODBYE")
                .unwrap_or(defaults.send_goodbye_after_ack),
        }
    }

    pub fn remote_endpoint(&self) -> String {
        format!("{}:{}", self.server_host, self.server_port)
    }
}

impl DebugTlsSettings {
    pub fn from_env() -> Self {
        Self {
            allow_insecure_certificate_validation: env_bool(
                "HOLOBRIDGE_TRANSPORT_ALLOW_INSECURE_CERT",
            )
            .unwrap_or(false),
        }
    }
}

fn env_bool(name: &str) -> Option<bool> {
    env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

fn env_u16(name: &str) -> Option<u16> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
}
