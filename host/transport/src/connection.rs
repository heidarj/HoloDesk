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
    UnexpectedMessage {
        role: ConnectionRole,
        message_type: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct ControlConnection {
    role: ConnectionRole,
    transcript: ConnectionTranscript,
    hello_received: bool,
    hello_ack_received: bool,
    goodbye_sent: bool,
    goodbye_received: bool,
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
        }
    }

    pub fn role(&self) -> ConnectionRole {
        self.role
    }

    pub fn on_receive(&mut self, message: ControlMessage) -> Result<Vec<ControlMessage>, ConnectionError> {
        self.transcript.received.push(message.clone());
        match self.role {
            ConnectionRole::Server => self.on_receive_as_server(message),
            ConnectionRole::Client => self.on_receive_as_client(message),
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

    pub fn handshake_complete(&self) -> bool {
        match self.role {
            ConnectionRole::Client => self.hello_ack_received,
            ConnectionRole::Server => self.hello_received,
        }
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
    ) -> Result<Vec<ControlMessage>, ConnectionError> {
        match message {
            ControlMessage::Hello { .. } => {
                if self.hello_received {
                    return Err(ConnectionError::DuplicateHello);
                }
                self.hello_received = true;
                let ack = ControlMessage::hello_ack("ok");
                self.transcript.sent.push(ack.clone());
                Ok(vec![ack])
            }
            ControlMessage::HelloAck { .. } => Err(ConnectionError::UnexpectedMessage {
                role: self.role,
                message_type: "hello_ack",
            }),
            ControlMessage::Goodbye { .. } => {
                self.goodbye_received = true;
                Ok(Vec::new())
            }
        }
    }

    fn on_receive_as_client(
        &mut self,
        message: ControlMessage,
    ) -> Result<Vec<ControlMessage>, ConnectionError> {
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
                Ok(Vec::new())
            }
            ControlMessage::Goodbye { .. } => {
                self.goodbye_received = true;
                Ok(Vec::new())
            }
        }
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
            Self::DuplicateHello => write!(formatter, "duplicate hello received on control stream"),
            Self::DuplicateHelloAck => write!(formatter, "duplicate hello_ack received on control stream"),
            Self::UnexpectedMessage { role, message_type } => write!(
                formatter,
                "unexpected {message_type} for {:?} control stream state",
                role
            ),
        }
    }
}

impl Error for ConnectionError {}

impl From<ProtocolError> for ConnectionError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}