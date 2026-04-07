use std::{convert::TryInto, error::Error, fmt};

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_ALPN: &str = "holobridge-m2";
pub const CONTROL_STREAM_CAPABILITY: &str = "control-stream-v1";
pub const POINTER_STREAM_CAPABILITY: &str = "pointer-stream-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    Hello {
        #[serde(rename = "protocol_version")]
        protocol_version: u32,
        #[serde(rename = "client_name")]
        client_name: String,
        capabilities: Vec<String>,
    },
    HelloAck {
        #[serde(rename = "protocol_version")]
        protocol_version: u32,
        message: String,
    },
    Goodbye {
        reason: String,
    },
    Authenticate {
        #[serde(rename = "identity_token")]
        identity_token: String,
    },
    ResumeSession {
        #[serde(rename = "resume_token")]
        resume_token: String,
    },
    AuthResult {
        success: bool,
        message: String,
        #[serde(rename = "user_display_name")]
        user_display_name: Option<String>,
        #[serde(rename = "session_id")]
        session_id: Option<String>,
        #[serde(rename = "resume_token")]
        resume_token: Option<String>,
        #[serde(rename = "resume_token_ttl_secs")]
        resume_token_ttl_secs: Option<u64>,
    },
    ResumeResult {
        success: bool,
        message: String,
        #[serde(rename = "user_display_name")]
        user_display_name: Option<String>,
        #[serde(rename = "session_id")]
        session_id: Option<String>,
        #[serde(rename = "resume_token")]
        resume_token: Option<String>,
        #[serde(rename = "resume_token_ttl_secs")]
        resume_token_ttl_secs: Option<u64>,
    },
    PointerShape {
        #[serde(rename = "shape_kind")]
        shape_kind: String,
        width: u32,
        height: u32,
        #[serde(rename = "hotspot_x")]
        hotspot_x: i32,
        #[serde(rename = "hotspot_y")]
        hotspot_y: i32,
        #[serde(rename = "pixels_rgba_base64")]
        pixels_rgba_base64: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    FrameTooShort { actual: usize },
    FrameTooLarge { actual: usize },
    LengthMismatch { declared: usize, actual: usize },
    InvalidJson(String),
    UnsupportedProtocolVersion { actual: u32 },
}

#[derive(Debug, Default, Clone)]
pub struct FrameAccumulator {
    buffer: Vec<u8>,
}

pub struct ControlMessageCodec;

impl ControlMessage {
    pub fn hello(client_name: impl Into<String>, capabilities: Vec<String>) -> Self {
        Self::Hello {
            protocol_version: PROTOCOL_VERSION,
            client_name: client_name.into(),
            capabilities,
        }
    }

    pub fn hello_smoke() -> Self {
        Self::hello(
            "transport-smoke",
            vec![CONTROL_STREAM_CAPABILITY.to_owned()],
        )
    }

    pub fn hello_ack(message: impl Into<String>) -> Self {
        Self::HelloAck {
            protocol_version: PROTOCOL_VERSION,
            message: message.into(),
        }
    }

    pub fn goodbye(reason: impl Into<String>) -> Self {
        Self::Goodbye {
            reason: reason.into(),
        }
    }

    pub fn authenticate(identity_token: impl Into<String>) -> Self {
        Self::Authenticate {
            identity_token: identity_token.into(),
        }
    }

    pub fn resume_session(resume_token: impl Into<String>) -> Self {
        Self::ResumeSession {
            resume_token: resume_token.into(),
        }
    }

    pub fn auth_result(
        success: bool,
        message: impl Into<String>,
        user_display_name: Option<String>,
        session_id: Option<String>,
        resume_token: Option<String>,
        resume_token_ttl_secs: Option<u64>,
    ) -> Self {
        Self::AuthResult {
            success,
            message: message.into(),
            user_display_name,
            session_id,
            resume_token,
            resume_token_ttl_secs,
        }
    }

    pub fn resume_result(
        success: bool,
        message: impl Into<String>,
        user_display_name: Option<String>,
        session_id: Option<String>,
        resume_token: Option<String>,
        resume_token_ttl_secs: Option<u64>,
    ) -> Self {
        Self::ResumeResult {
            success,
            message: message.into(),
            user_display_name,
            session_id,
            resume_token,
            resume_token_ttl_secs,
        }
    }

    pub fn pointer_shape(
        shape_kind: impl Into<String>,
        width: u32,
        height: u32,
        hotspot_x: i32,
        hotspot_y: i32,
        pixels_rgba_base64: impl Into<String>,
    ) -> Self {
        Self::PointerShape {
            shape_kind: shape_kind.into(),
            width,
            height,
            hotspot_x,
            hotspot_y,
            pixels_rgba_base64: pixels_rgba_base64.into(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::HelloAck { .. } => "hello_ack",
            Self::Goodbye { .. } => "goodbye",
            Self::Authenticate { .. } => "authenticate",
            Self::ResumeSession { .. } => "resume_session",
            Self::AuthResult { .. } => "auth_result",
            Self::ResumeResult { .. } => "resume_result",
            Self::PointerShape { .. } => "pointer_shape",
        }
    }

    pub fn protocol_version(&self) -> Option<u32> {
        match self {
            Self::Hello {
                protocol_version, ..
            }
            | Self::HelloAck {
                protocol_version, ..
            } => Some(*protocol_version),
            Self::Goodbye { .. }
            | Self::Authenticate { .. }
            | Self::ResumeSession { .. }
            | Self::AuthResult { .. }
            | Self::ResumeResult { .. }
            | Self::PointerShape { .. } => None,
        }
    }
}

impl ControlMessageCodec {
    pub fn encode(message: &ControlMessage) -> Result<Vec<u8>, ProtocolError> {
        let payload = serde_json::to_vec(message)
            .map_err(|error| ProtocolError::InvalidJson(error.to_string()))?;
        let payload_len: u32 =
            payload
                .len()
                .try_into()
                .map_err(|_| ProtocolError::FrameTooLarge {
                    actual: payload.len(),
                })?;

        let mut encoded = Vec::with_capacity(4 + payload.len());
        encoded.extend_from_slice(&payload_len.to_be_bytes());
        encoded.extend_from_slice(&payload);
        Ok(encoded)
    }

    pub fn decode_frame(frame: &[u8]) -> Result<ControlMessage, ProtocolError> {
        if frame.len() < 4 {
            return Err(ProtocolError::FrameTooShort {
                actual: frame.len(),
            });
        }

        let declared = u32::from_be_bytes(frame[0..4].try_into().expect("length prefix")) as usize;
        let payload = &frame[4..];

        if declared != payload.len() {
            return Err(ProtocolError::LengthMismatch {
                declared,
                actual: payload.len(),
            });
        }

        let message: ControlMessage = serde_json::from_slice(payload)
            .map_err(|error| ProtocolError::InvalidJson(error.to_string()))?;
        validate_protocol_version(&message)?;
        Ok(message)
    }
}

impl FrameAccumulator {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    pub fn drain_messages(&mut self) -> Result<Vec<ControlMessage>, ProtocolError> {
        let mut messages = Vec::new();
        while let Some(message) = self.next_message()? {
            messages.push(message);
        }
        Ok(messages)
    }

    pub fn next_message(&mut self) -> Result<Option<ControlMessage>, ProtocolError> {
        if self.buffer.len() < 4 {
            return Ok(None);
        }

        let declared =
            u32::from_be_bytes(self.buffer[0..4].try_into().expect("length prefix")) as usize;
        if self.buffer.len() < 4 + declared {
            return Ok(None);
        }

        let frame = self.buffer.drain(0..(4 + declared)).collect::<Vec<u8>>();
        ControlMessageCodec::decode_frame(&frame).map(Some)
    }
}

fn validate_protocol_version(message: &ControlMessage) -> Result<(), ProtocolError> {
    match message.protocol_version() {
        Some(PROTOCOL_VERSION) | None => Ok(()),
        Some(actual) => Err(ProtocolError::UnsupportedProtocolVersion { actual }),
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooShort { actual } => {
                write!(
                    formatter,
                    "frame shorter than 4-byte prefix: {actual} bytes"
                )
            }
            Self::FrameTooLarge { actual } => {
                write!(
                    formatter,
                    "frame payload too large to encode: {actual} bytes"
                )
            }
            Self::LengthMismatch { declared, actual } => {
                write!(
                    formatter,
                    "frame length mismatch: declared {declared}, actual {actual}"
                )
            }
            Self::InvalidJson(error) => write!(formatter, "invalid control message json: {error}"),
            Self::UnsupportedProtocolVersion { actual } => {
                write!(formatter, "unsupported protocol version: {actual}")
            }
        }
    }
}

impl Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_shape_roundtrip_preserves_shape_payload() {
        let message = ControlMessage::pointer_shape(
            "color",
            32,
            16,
            4,
            7,
            "AQIDBA==",
        );

        let encoded = ControlMessageCodec::encode(&message).unwrap();
        let decoded = ControlMessageCodec::decode_frame(&encoded).unwrap();

        assert_eq!(decoded, message);
        assert_eq!(decoded.protocol_version(), None);
    }
}
