use std::{error::Error, fmt};

use crate::protocol::{ControlMessage, ProtocolError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionRole {
    Client,
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseInitiator {
    None,
    Client,
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionTranscript {
    pub sent: Vec<ControlMessage>,
    pub received: Vec<ControlMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionError {
    Protocol(ProtocolError),
    DuplicateHello,
    DuplicateHelloAck,
    DuplicateAuthenticate,
    DuplicateResumeSession,
    DuplicateAuthResult,
    DuplicateResumeResult,
    ConflictingHandshakeMessage,
    UnexpectedMessage {
        role: ConnectionRole,
        message_type: &'static str,
    },
    AuthNotComplete,
}

/// Result of handshake processing, returned to the caller to drive async validation.
#[derive(Debug, Clone)]
pub enum HandshakeAction {
    /// The server received an Authenticate message; the caller should validate the token
    /// and then call `record_auth_result()`.
    ValidateToken(String),
    /// The server received a ResumeSession message; the caller should validate the token
    /// and then call `record_resume_result()`.
    ValidateResumeToken(String),
}

#[derive(Debug, Clone)]
pub struct ControlConnection {
    role: ConnectionRole,
    transcript: ConnectionTranscript,
    hello_received: bool,
    hello_ack_received: bool,
    goodbye_sent: bool,
    goodbye_received: bool,
    auth_received: bool,
    resume_received: bool,
    auth_result_sent: bool,
    auth_result_received: bool,
    resume_result_sent: bool,
    resume_result_received: bool,
    auth_success: Option<bool>,
}

impl ControlConnection {
    pub fn new(role: ConnectionRole) -> Self {
        Self {
            role,
            transcript: ConnectionTranscript {
                sent: Vec::new(),
                received: Vec::new(),
            },
            hello_received: false,
            hello_ack_received: false,
            goodbye_sent: false,
            goodbye_received: false,
            auth_received: false,
            resume_received: false,
            auth_result_sent: false,
            auth_result_received: false,
            resume_result_sent: false,
            resume_result_received: false,
            auth_success: None,
        }
    }

    pub fn role(&self) -> ConnectionRole {
        self.role
    }

    pub fn on_receive(
        &mut self,
        message: ControlMessage,
    ) -> Result<(Vec<ControlMessage>, Option<HandshakeAction>), ConnectionError> {
        self.transcript.received.push(message.clone());
        match self.role {
            ConnectionRole::Server => self.on_receive_as_server(message),
            ConnectionRole::Client => self.on_receive_as_client(message),
        }
    }

    /// Record the outcome of external auth validation (called by server after token validation).
    /// Returns the AuthResult message to send to the client.
    pub fn record_auth_result(
        &mut self,
        success: bool,
        message: impl Into<String>,
        user_display_name: Option<String>,
        session_id: Option<String>,
        resume_token: Option<String>,
        resume_token_ttl_secs: Option<u64>,
    ) -> ControlMessage {
        self.auth_success = Some(success);
        self.auth_result_sent = true;
        let msg = ControlMessage::auth_result(
            success,
            message,
            user_display_name,
            session_id,
            resume_token,
            resume_token_ttl_secs,
        );
        self.transcript.sent.push(msg.clone());
        msg
    }

    pub fn record_resume_result(
        &mut self,
        success: bool,
        message: impl Into<String>,
        user_display_name: Option<String>,
        session_id: Option<String>,
        resume_token: Option<String>,
        resume_token_ttl_secs: Option<u64>,
    ) -> ControlMessage {
        self.auth_success = Some(success);
        self.resume_result_sent = true;
        let msg = ControlMessage::resume_result(
            success,
            message,
            user_display_name,
            session_id,
            resume_token,
            resume_token_ttl_secs,
        );
        self.transcript.sent.push(msg.clone());
        msg
    }

    pub fn session_established(&self) -> bool {
        match self.role {
            ConnectionRole::Server => {
                (self.auth_result_sent || self.resume_result_sent)
                    && self.auth_success == Some(true)
            }
            ConnectionRole::Client => {
                (self.auth_result_received || self.resume_result_received)
                    && self.auth_success == Some(true)
            }
        }
    }

    pub fn handshake_finished(&self) -> bool {
        match self.role {
            ConnectionRole::Server => self.auth_result_sent || self.resume_result_sent,
            ConnectionRole::Client => self.auth_result_received || self.resume_result_received,
        }
    }

    pub fn record_outbound(&mut self, message: ControlMessage) {
        if matches!(message, ControlMessage::Goodbye { .. }) {
            self.goodbye_sent = true;
        }
        self.transcript.sent.push(message);
    }

    pub fn initiate_goodbye(&mut self, reason: impl Into<String>) -> ControlMessage {
        let message = ControlMessage::goodbye(reason);
        self.goodbye_sent = true;
        self.transcript.sent.push(message.clone());
        message
    }

    pub fn orderly_shutdown_complete(&self) -> bool {
        self.goodbye_sent || self.goodbye_received
    }

    /// Returns true when the basic Hello/HelloAck exchange is complete (pre-auth).
    pub fn hello_exchanged(&self) -> bool {
        match self.role {
            ConnectionRole::Client => self.hello_ack_received,
            ConnectionRole::Server => self.hello_received,
        }
    }

    /// Returns true when the full handshake (Hello + Auth) is complete.
    pub fn handshake_complete(&self) -> bool {
        self.hello_exchanged() && self.handshake_finished()
    }

    pub fn close_initiator(&self) -> CloseInitiator {
        match (self.goodbye_sent, self.goodbye_received, self.role) {
            (false, false, _) => CloseInitiator::None,
            (true, false, ConnectionRole::Client) | (true, true, ConnectionRole::Client) => {
                CloseInitiator::Client
            }
            (true, false, ConnectionRole::Server) | (true, true, ConnectionRole::Server) => {
                CloseInitiator::Server
            }
            (false, true, ConnectionRole::Client) => CloseInitiator::Server,
            (false, true, ConnectionRole::Server) => CloseInitiator::Client,
        }
    }

    pub fn transcript(&self) -> &ConnectionTranscript {
        &self.transcript
    }

    fn on_receive_as_server(
        &mut self,
        message: ControlMessage,
    ) -> Result<(Vec<ControlMessage>, Option<HandshakeAction>), ConnectionError> {
        match message {
            ControlMessage::Hello { .. } => {
                if self.hello_received {
                    return Err(ConnectionError::DuplicateHello);
                }
                self.hello_received = true;
                let ack = ControlMessage::hello_ack("ok");
                self.transcript.sent.push(ack.clone());
                Ok((vec![ack], None))
            }
            ControlMessage::HelloAck { .. } => Err(ConnectionError::UnexpectedMessage {
                role: self.role,
                message_type: "hello_ack",
            }),
            ControlMessage::Authenticate { identity_token } => {
                if !self.hello_received {
                    return Err(ConnectionError::UnexpectedMessage {
                        role: self.role,
                        message_type: "authenticate (before hello)",
                    });
                }
                if self.resume_received {
                    return Err(ConnectionError::ConflictingHandshakeMessage);
                }
                if self.auth_received {
                    return Err(ConnectionError::DuplicateAuthenticate);
                }
                self.auth_received = true;
                Ok((
                    Vec::new(),
                    Some(HandshakeAction::ValidateToken(identity_token)),
                ))
            }
            ControlMessage::ResumeSession { resume_token } => {
                if !self.hello_received {
                    return Err(ConnectionError::UnexpectedMessage {
                        role: self.role,
                        message_type: "resume_session (before hello)",
                    });
                }
                if self.auth_received {
                    return Err(ConnectionError::ConflictingHandshakeMessage);
                }
                if self.resume_received {
                    return Err(ConnectionError::DuplicateResumeSession);
                }
                self.resume_received = true;
                Ok((
                    Vec::new(),
                    Some(HandshakeAction::ValidateResumeToken(resume_token)),
                ))
            }
            ControlMessage::AuthResult { .. } => Err(ConnectionError::UnexpectedMessage {
                role: self.role,
                message_type: "auth_result",
            }),
            ControlMessage::ResumeResult { .. } => Err(ConnectionError::UnexpectedMessage {
                role: self.role,
                message_type: "resume_result",
            }),
            ControlMessage::Goodbye { .. } => {
                self.goodbye_received = true;
                Ok((Vec::new(), None))
            }
        }
    }

    fn on_receive_as_client(
        &mut self,
        message: ControlMessage,
    ) -> Result<(Vec<ControlMessage>, Option<HandshakeAction>), ConnectionError> {
        match message {
            ControlMessage::Hello { .. } => Err(ConnectionError::UnexpectedMessage {
                role: self.role,
                message_type: "hello",
            }),
            ControlMessage::HelloAck { .. } => {
                if self.hello_ack_received {
                    return Err(ConnectionError::DuplicateHelloAck);
                }
                self.hello_ack_received = true;
                Ok((Vec::new(), None))
            }
            ControlMessage::Authenticate { .. } => Err(ConnectionError::UnexpectedMessage {
                role: self.role,
                message_type: "authenticate",
            }),
            ControlMessage::ResumeSession { .. } => Err(ConnectionError::UnexpectedMessage {
                role: self.role,
                message_type: "resume_session",
            }),
            ControlMessage::AuthResult { success, .. } => {
                if self.auth_result_received {
                    return Err(ConnectionError::DuplicateAuthResult);
                }
                self.auth_result_received = true;
                self.auth_success = Some(success);
                Ok((Vec::new(), None))
            }
            ControlMessage::ResumeResult { success, .. } => {
                if self.resume_result_received {
                    return Err(ConnectionError::DuplicateResumeResult);
                }
                self.resume_result_received = true;
                self.auth_success = Some(success);
                Ok((Vec::new(), None))
            }
            ControlMessage::Goodbye { .. } => {
                self.goodbye_received = true;
                Ok((Vec::new(), None))
            }
        }
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
            Self::DuplicateHello => write!(formatter, "duplicate hello received on control stream"),
            Self::DuplicateHelloAck => {
                write!(formatter, "duplicate hello_ack received on control stream")
            }
            Self::DuplicateAuthenticate => {
                write!(
                    formatter,
                    "duplicate authenticate received on control stream"
                )
            }
            Self::DuplicateResumeSession => {
                write!(
                    formatter,
                    "duplicate resume_session received on control stream"
                )
            }
            Self::DuplicateAuthResult => {
                write!(
                    formatter,
                    "duplicate auth_result received on control stream"
                )
            }
            Self::DuplicateResumeResult => {
                write!(
                    formatter,
                    "duplicate resume_result received on control stream"
                )
            }
            Self::ConflictingHandshakeMessage => {
                write!(
                    formatter,
                    "conflicting authenticate/resume handshake messages received"
                )
            }
            Self::UnexpectedMessage { role, message_type } => write!(
                formatter,
                "unexpected {message_type} for {:?} control stream state",
                role
            ),
            Self::AuthNotComplete => write!(formatter, "auth handshake not complete"),
        }
    }
}

impl Error for ConnectionError {}

impl From<ProtocolError> for ConnectionError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}
