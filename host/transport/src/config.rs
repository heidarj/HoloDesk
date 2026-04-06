use std::{env, time::Duration};

use crate::protocol::DEFAULT_ALPN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertificateSource {
    SelfSigned,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DebugTlsSettings {
    pub allow_insecure_certificate_validation: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VideoSource {
    #[default]
    DesktopCapture,
    SyntheticLoopback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticVideoPreset {
    TransportLoopbackV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticAccessUnit {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
    pub pts_100ns: i64,
    pub duration_100ns: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoStreamConfig {
    pub enabled: bool,
    pub source: VideoSource,
    pub display_id: Option<String>,
    pub datagram_payload_cap_bytes: usize,
    pub datagram_receive_buffer_size: Option<usize>,
    pub datagram_send_buffer_size: usize,
    pub capture_timeout_ms: u32,
    pub first_frame_timeout_secs: u64,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub bitrate_bps: Option<u32>,
    pub synthetic_preset: Option<SyntheticVideoPreset>,
    pub synthetic_access_units: Option<Vec<SyntheticAccessUnit>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub alpn: String,
    pub certificate: CertificateSource,
    pub debug_validation: DebugTlsSettings,
    pub server_initiated_close_after_ack: bool,
    pub server_wait_timeout: Option<Duration>,
    pub video: VideoStreamConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportClientConfig {
    pub server_host: String,
    pub server_port: u16,
    pub server_name: Option<String>,
    pub alpn: String,
    pub debug_validation: DebugTlsSettings,
    pub send_goodbye_after_ack: bool,
    /// Identity token to send during auth handshake (for smoke testing).
    pub identity_token: Option<String>,
    /// Resume token to send during resume handshake (for smoke testing).
    pub resume_token: Option<String>,
    /// Whether the client should advertise media datagram support.
    pub request_video_stream: bool,
    pub datagram_receive_buffer_size: Option<usize>,
    pub datagram_send_buffer_size: usize,
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
            server_wait_timeout: None,
            video: VideoStreamConfig::default(),
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
            identity_token: None,
            resume_token: None,
            request_video_stream: false,
            datagram_receive_buffer_size: Some(1024 * 1024),
            datagram_send_buffer_size: 1024 * 1024,
        }
    }
}

impl Default for VideoStreamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            source: VideoSource::DesktopCapture,
            display_id: None,
            datagram_payload_cap_bytes: 1_100,
            datagram_receive_buffer_size: Some(1024 * 1024),
            datagram_send_buffer_size: 1024 * 1024,
            capture_timeout_ms: 16,
            first_frame_timeout_secs: 2,
            frame_rate_num: 60,
            frame_rate_den: 1,
            bitrate_bps: None,
            synthetic_preset: None,
            synthetic_access_units: None,
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
            server_initiated_close_after_ack: env_bool(
                "HOLOBRIDGE_TRANSPORT_SERVER_CLOSE_AFTER_ACK",
            )
            .unwrap_or(defaults.server_initiated_close_after_ack),
            server_wait_timeout: env_optional_duration_secs(
                "HOLOBRIDGE_TRANSPORT_SERVER_WAIT_TIMEOUT_SECS",
            )
            .unwrap_or(defaults.server_wait_timeout),
            video: VideoStreamConfig::from_env(),
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
            identity_token: env::var("HOLOBRIDGE_AUTH_IDENTITY_TOKEN").ok(),
            resume_token: env::var("HOLOBRIDGE_AUTH_RESUME_TOKEN").ok(),
            request_video_stream: env_bool("HOLOBRIDGE_VIDEO_REQUEST")
                .unwrap_or(defaults.request_video_stream),
            datagram_receive_buffer_size: env_optional_usize(
                "HOLOBRIDGE_DATAGRAM_RECV_BUFFER_BYTES",
            )
            .unwrap_or(defaults.datagram_receive_buffer_size),
            datagram_send_buffer_size: env_usize("HOLOBRIDGE_DATAGRAM_SEND_BUFFER_BYTES")
                .unwrap_or(defaults.datagram_send_buffer_size),
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

impl VideoStreamConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        let source = env_video_source("HOLOBRIDGE_VIDEO_SOURCE").unwrap_or(defaults.source);
        let (frame_rate_num, frame_rate_den) = env_frame_rate("HOLOBRIDGE_VIDEO_FRAME_RATE")
            .unwrap_or((defaults.frame_rate_num, defaults.frame_rate_den));

        Self {
            enabled: env_bool("HOLOBRIDGE_VIDEO_ENABLED").unwrap_or(defaults.enabled),
            source,
            display_id: env::var("HOLOBRIDGE_VIDEO_DISPLAY_ID").ok(),
            datagram_payload_cap_bytes: env_usize("HOLOBRIDGE_VIDEO_DATAGRAM_PAYLOAD_CAP_BYTES")
                .unwrap_or(defaults.datagram_payload_cap_bytes),
            datagram_receive_buffer_size: env_optional_usize(
                "HOLOBRIDGE_VIDEO_DATAGRAM_RECV_BUFFER_BYTES",
            )
            .unwrap_or(defaults.datagram_receive_buffer_size),
            datagram_send_buffer_size: env_usize("HOLOBRIDGE_VIDEO_DATAGRAM_SEND_BUFFER_BYTES")
                .unwrap_or(defaults.datagram_send_buffer_size),
            capture_timeout_ms: env_u32("HOLOBRIDGE_VIDEO_CAPTURE_TIMEOUT_MS")
                .unwrap_or(defaults.capture_timeout_ms),
            first_frame_timeout_secs: env_u64("HOLOBRIDGE_VIDEO_FIRST_FRAME_TIMEOUT_SECS")
                .unwrap_or(defaults.first_frame_timeout_secs),
            frame_rate_num,
            frame_rate_den,
            bitrate_bps: env_u32("HOLOBRIDGE_VIDEO_BITRATE_BPS")
                .or(defaults.bitrate_bps),
            synthetic_preset: env_synthetic_video_preset("HOLOBRIDGE_VIDEO_SYNTHETIC_PRESET")
                .or_else(|| {
                    if source == VideoSource::SyntheticLoopback {
                        Some(SyntheticVideoPreset::TransportLoopbackV1)
                    } else {
                        defaults.synthetic_preset
                    }
                }),
            synthetic_access_units: None,
        }
    }

    pub fn resolved_synthetic_access_units(&self) -> Option<Vec<SyntheticAccessUnit>> {
        if let Some(access_units) = &self.synthetic_access_units {
            return Some(access_units.clone());
        }

        match self.source {
            VideoSource::DesktopCapture => None,
            VideoSource::SyntheticLoopback => Some(
                self.synthetic_preset
                    .unwrap_or(SyntheticVideoPreset::TransportLoopbackV1)
                    .build_access_units(self.frame_rate_num, self.frame_rate_den),
            ),
        }
    }
}

impl SyntheticVideoPreset {
    pub fn build_access_units(
        self,
        frame_rate_num: u32,
        frame_rate_den: u32,
    ) -> Vec<SyntheticAccessUnit> {
        let frame_duration_100ns = frame_duration_100ns(frame_rate_num, frame_rate_den);

        match self {
            Self::TransportLoopbackV1 => vec![SyntheticAccessUnit {
                data: transport_loopback_payload(),
                is_keyframe: true,
                pts_100ns: frame_duration_100ns,
                duration_100ns: frame_duration_100ns,
            }],
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

fn env_u32(name: &str) -> Option<u32> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
}

fn env_u64(name: &str) -> Option<u64> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn env_usize(name: &str) -> Option<usize> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
}

fn env_video_source(name: &str) -> Option<VideoSource> {
    env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "desktop" => Some(VideoSource::DesktopCapture),
            "synthetic" => Some(VideoSource::SyntheticLoopback),
            _ => None,
        })
}

fn env_synthetic_video_preset(name: &str) -> Option<SyntheticVideoPreset> {
    env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "transport-loopback-v1" => Some(SyntheticVideoPreset::TransportLoopbackV1),
            _ => None,
        })
}

fn env_optional_duration_secs(name: &str) -> Option<Option<Duration>> {
    env::var(name).ok().and_then(|value| {
        value.trim().parse::<u64>().ok().map(|seconds| {
            if seconds == 0 {
                None
            } else {
                Some(Duration::from_secs(seconds))
            }
        })
    })
}

fn env_optional_usize(name: &str) -> Option<Option<usize>> {
    env::var(name).ok().and_then(|value| {
        value.trim().parse::<usize>().ok().map(|bytes| {
            if bytes == 0 {
                None
            } else {
                Some(bytes)
            }
        })
    })
}

fn env_frame_rate(name: &str) -> Option<(u32, u32)> {
    let value = env::var(name).ok()?;
    let (num, den) = value.trim().split_once('/')?;
    let num = num.parse::<u32>().ok()?;
    let den = den.parse::<u32>().ok()?;
    if num == 0 || den == 0 {
        return None;
    }
    Some((num, den))
}

fn frame_duration_100ns(frame_rate_num: u32, frame_rate_den: u32) -> i64 {
    if frame_rate_num == 0 || frame_rate_den == 0 {
        return 166_666;
    }

    ((frame_rate_den as u128 * 10_000_000u128) / frame_rate_num as u128) as i64
}

fn transport_loopback_payload() -> Vec<u8> {
    (0..2_800u32)
        .map(|index| ((index % 251) as u8).wrapping_add(1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_loopback_source_resolves_default_preset() {
        let config = VideoStreamConfig {
            enabled: true,
            source: VideoSource::SyntheticLoopback,
            frame_rate_num: 60,
            frame_rate_den: 1,
            ..VideoStreamConfig::default()
        };

        let access_units = config
            .resolved_synthetic_access_units()
            .expect("synthetic access units");

        assert_eq!(access_units.len(), 1);
        assert!(access_units[0].is_keyframe);
        assert_eq!(access_units[0].data.len(), 2_800);
        assert_eq!(access_units[0].duration_100ns, 166_666);
    }

    #[test]
    fn explicit_synthetic_access_units_override_preset_selection() {
        let config = VideoStreamConfig {
            enabled: true,
            source: VideoSource::SyntheticLoopback,
            synthetic_access_units: Some(vec![SyntheticAccessUnit {
                data: vec![1, 2, 3],
                is_keyframe: false,
                pts_100ns: 42,
                duration_100ns: 42,
            }]),
            ..VideoStreamConfig::default()
        };

        let access_units = config
            .resolved_synthetic_access_units()
            .expect("synthetic access units");

        assert_eq!(access_units.len(), 1);
        assert_eq!(access_units[0].data, vec![1, 2, 3]);
        assert!(!access_units[0].is_keyframe);
        assert_eq!(access_units[0].duration_100ns, 42);
    }
}
