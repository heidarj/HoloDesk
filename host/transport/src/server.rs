use std::{error::Error, fmt, net::SocketAddr, sync::Arc, time::Duration};

use quinn::{Endpoint, RecvStream, SendStream};
use tokio::time::timeout;
use tracing::{info, warn};

use holobridge_auth::{AuthConfig, AuthError, AuthorizedUserStore, TokenValidator};

use crate::{
    config::{TransportClientConfig, TransportServerConfig},
    connection::{AuthAction, ConnectionError, ConnectionRole, ControlConnection},
    protocol::{ControlMessage, ControlMessageCodec, FrameAccumulator, ProtocolError},
    tls::{build_client_config, build_server_config, TlsConfigError},
};

#[derive(Debug)]
pub enum TransportError {
    Tls(TlsConfigError),
    Quinn(quinn::ConnectionError),
    Connect(quinn::ConnectError),
    WriteError(quinn::WriteError),
    ReadError(quinn::ReadExactError),
    ClosedStream(quinn::ClosedStream),
    Io(std::io::Error),
    Protocol(ProtocolError),
    Connection(ConnectionError),
    InvalidEndpoint(String),
    Timeout(&'static str),
    Runtime(String),
    Auth(AuthError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRuntimeSummary {
    pub backend: &'static str,
    pub bind_endpoint: String,
    pub alpn: String,
    pub certificate: String,
    pub close_mode: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmokeClientRuntimeSummary {
    pub remote_endpoint: String,
    pub alpn: String,
    pub validation: String,
    pub close_mode: &'static str,
}

pub struct TransportServer {
    config: TransportServerConfig,
    auth_validator: Option<Arc<TokenValidator>>,
    user_store: Option<Arc<AuthorizedUserStore>>,
}

#[derive(Debug, Clone)]
pub struct TransportSmokeClient {
    config: TransportClientConfig,
}

const CLIENT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

impl TransportServer {
    pub fn new(config: TransportServerConfig) -> Self {
        Self {
            config,
            auth_validator: None,
            user_store: None,
        }
    }

    /// Create a server with auth validation enabled.
    pub async fn with_auth(
        config: TransportServerConfig,
        auth_config: &AuthConfig,
    ) -> Result<Self, TransportError> {
        let validator = TokenValidator::new(auth_config)
            .await
            .map_err(TransportError::Auth)?;
        let user_store = AuthorizedUserStore::load(
            &auth_config.user_store_path,
            auth_config.bootstrap_mode,
        )
        .await
        .map_err(TransportError::Auth)?;

        Ok(Self {
            config,
            auth_validator: Some(Arc::new(validator)),
            user_store: Some(Arc::new(user_store)),
        })
    }

    pub fn config(&self) -> &TransportServerConfig {
        &self.config
    }

    pub fn runtime_summary(&self) -> ServerRuntimeSummary {
        ServerRuntimeSummary {
            backend: "quinn",
            bind_endpoint: self.config.listen_endpoint(),
            alpn: self.config.alpn.clone(),
            certificate: "self-signed (rcgen)".to_owned(),
            close_mode: if self.config.server_initiated_close_after_ack {
                "server-initiated"
            } else {
                "client-initiated"
            },
        }
    }

    pub async fn serve_once(&self) -> Result<(), TransportError> {
        let server_config = build_server_config(&self.config)?;
        let bind_addr: SocketAddr = self
            .config
            .listen_endpoint()
            .parse()
            .map_err(|_| TransportError::InvalidEndpoint(self.config.listen_endpoint()))?;
        let endpoint = Endpoint::server(server_config, bind_addr)?;

        info!(endpoint = %bind_addr, alpn = %self.config.alpn, "host transport listener started");

        let incoming = await_with_optional_timeout(
            self.config.server_wait_timeout,
            endpoint.accept(),
            "timed out waiting for incoming connection",
        )
        .await?
        .ok_or_else(|| TransportError::Runtime("endpoint closed before accepting".to_owned()))?;

        let connection = incoming.await.map_err(TransportError::Quinn)?;
        let remote = connection.remote_address();
        info!(remote = %remote, "host transport connection established");

        let (send, recv) = await_with_optional_timeout(
            self.config.server_wait_timeout,
            connection.accept_bi(),
            "timed out waiting for control stream",
        )
        .await?
        .map_err(TransportError::Quinn)?;

        info!("host transport control stream accepted");

        let result = run_server_control_stream(
            send,
            recv,
            self.config.server_initiated_close_after_ack,
            self.auth_validator.clone(),
            self.user_store.clone(),
        )
        .await;

        connection.close(quinn::VarInt::from_u32(0), b"done");
        endpoint.close(quinn::VarInt::from_u32(0), b"done");
        endpoint.wait_idle().await;

        result
    }
}

impl TransportSmokeClient {
    pub fn new(config: TransportClientConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &TransportClientConfig {
        &self.config
    }

    pub fn runtime_summary(&self) -> SmokeClientRuntimeSummary {
        SmokeClientRuntimeSummary {
            remote_endpoint: self.config.remote_endpoint(),
            alpn: self.config.alpn.clone(),
            validation: if self.config.debug_validation.allow_insecure_certificate_validation {
                "debug-insecure (certificate verification bypassed)".to_owned()
            } else {
                "system-trust".to_owned()
            },
            close_mode: if self.config.send_goodbye_after_ack {
                "client-initiated"
            } else {
                "server-initiated"
            },
        }
    }

    pub async fn run(&self) -> Result<(), TransportError> {
        let client_config = build_client_config(&self.config)?;
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
        endpoint.set_default_client_config(client_config);

        let server_addr: SocketAddr = self
            .config
            .remote_endpoint()
            .parse()
            .map_err(|_| TransportError::InvalidEndpoint(self.config.remote_endpoint()))?;
        let server_name = self
            .config
            .server_name
            .clone()
            .unwrap_or_else(|| self.config.server_host.clone());

        info!(endpoint = %server_addr, server_name = %server_name, alpn = %self.config.alpn, "transport smoke client connecting");

        let connection = timeout(
            CLIENT_WAIT_TIMEOUT,
            endpoint.connect(server_addr, &server_name)?,
        )
        .await
        .map_err(|_| TransportError::Timeout("timed out connecting to server"))?
        .map_err(TransportError::Quinn)?;

        let remote = connection.remote_address();
        info!(remote = %remote, "transport smoke client connected");

        let (send, recv) = connection.open_bi().await.map_err(TransportError::Quinn)?;
        info!("transport smoke client opened control stream");

        let result = run_client_control_stream(
            send,
            recv,
            self.config.send_goodbye_after_ack,
            self.config.identity_token.as_deref(),
        )
        .await;

        connection.close(quinn::VarInt::from_u32(0), b"done");
        endpoint.wait_idle().await;

        result
    }
}

async fn run_server_control_stream(
    mut send: SendStream,
    mut recv: RecvStream,
    server_initiated_close: bool,
    auth_validator: Option<Arc<TokenValidator>>,
    user_store: Option<Arc<AuthorizedUserStore>>,
) -> Result<(), TransportError> {
    let mut protocol = ControlConnection::new(ConnectionRole::Server);
    let mut accumulator = FrameAccumulator::default();

    // Read hello from client
    let messages = read_messages(&mut recv, &mut accumulator).await?;
    for message in messages {
        info!(kind = message.kind(), "host transport received control message");
        let (responses, _auth_action) = protocol.on_receive(message)?;
        for response in &responses {
            info!(kind = response.kind(), "host transport sending control message");
            send_message(&mut send, response).await?;
        }
    }

    // If auth is configured, wait for Authenticate message
    if auth_validator.is_some() {
        info!("host transport waiting for authentication");
        let auth_messages = read_messages(&mut recv, &mut accumulator).await?;
        for message in auth_messages {
            info!(kind = message.kind(), "host transport received control message");
            let (_responses, auth_action) = protocol.on_receive(message)?;

            if let Some(AuthAction::ValidateToken(token)) = auth_action {
                let validator = auth_validator.as_ref().unwrap();
                let store = user_store.as_ref().unwrap();

                match validator.validate(&token).await {
                    Ok(claims) => {
                        let sub = &claims.sub;
                        let authorized = store
                            .check_or_bootstrap(sub, claims.email.as_deref())
                            .await
                            .map_err(TransportError::Auth)?;

                        if authorized {
                            info!(sub, "auth succeeded");
                            let result = protocol.record_auth_result(
                                true,
                                "authenticated",
                                claims.email.clone(),
                            );
                            send_message(&mut send, &result).await?;
                        } else {
                            warn!(sub, "auth failed: user not authorized");
                            let result = protocol.record_auth_result(
                                false,
                                "user not authorized",
                                None,
                            );
                            send_message(&mut send, &result).await?;
                            send.finish()?;
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "auth failed: token validation error");
                        let result =
                            protocol.record_auth_result(false, e.to_string(), None);
                        send_message(&mut send, &result).await?;
                        send.finish()?;
                        return Ok(());
                    }
                }
            }
        }
    }

    if server_initiated_close && protocol.hello_exchanged() {
        let goodbye = protocol.initiate_goodbye("server-initiated-close");
        info!(kind = goodbye.kind(), "host transport sending control message");
        send_message(&mut send, &goodbye).await?;
        send.finish()?;
        info!("host transport finished send side");
    }

    // Wait for client goodbye or stream close
    loop {
        match read_messages(&mut recv, &mut accumulator).await {
            Ok(messages) if messages.is_empty() => {
                info!("host transport control stream read finished (peer closed)");
                break;
            }
            Ok(messages) => {
                for message in messages {
                    info!(kind = message.kind(), "host transport received control message");
                    protocol.on_receive(message)?;
                }
                if protocol.orderly_shutdown_complete() {
                    info!("host transport orderly shutdown complete");
                    break;
                }
            }
            Err(_) if protocol.orderly_shutdown_complete() => {
                info!("host transport control stream closed after orderly shutdown");
                break;
            }
            Err(TransportError::ReadError(quinn::ReadExactError::FinishedEarly(_))) => {
                info!("host transport control stream finished");
                break;
            }
            Err(e) => return Err(e),
        }
    }

    if !server_initiated_close {
        send.finish()?;
    }

    info!(
        handshake_complete = protocol.handshake_complete(),
        orderly_shutdown = protocol.orderly_shutdown_complete(),
        "host transport session complete"
    );
    Ok(())
}

async fn run_client_control_stream(
    mut send: SendStream,
    mut recv: RecvStream,
    send_goodbye_after_ack: bool,
    identity_token: Option<&str>,
) -> Result<(), TransportError> {
    let mut protocol = ControlConnection::new(ConnectionRole::Client);
    let mut accumulator = FrameAccumulator::default();

    // Send hello
    let hello = ControlMessage::hello_smoke();
    protocol.record_outbound(hello.clone());
    info!(kind = hello.kind(), "transport smoke client sending control message");
    send_message(&mut send, &hello).await?;

    // Read hello_ack (and possibly goodbye)
    let messages = read_messages(&mut recv, &mut accumulator).await?;
    for message in messages {
        info!(kind = message.kind(), "transport smoke client received control message");
        protocol.on_receive(message)?;
    }

    // If we have an identity token, send Authenticate after HelloAck
    if let Some(token) = identity_token {
        if protocol.hello_exchanged() {
            let auth = ControlMessage::authenticate(token);
            protocol.record_outbound(auth.clone());
            info!(kind = auth.kind(), "transport smoke client sending control message");
            send_message(&mut send, &auth).await?;

            // Read AuthResult
            let auth_messages = read_messages(&mut recv, &mut accumulator).await?;
            for message in auth_messages {
                info!(kind = message.kind(), "transport smoke client received control message");
                protocol.on_receive(message)?;
            }

            if !protocol.auth_complete() {
                info!("transport smoke client auth was rejected");
                send.finish()?;
                return Ok(());
            }
        }
    }

    if send_goodbye_after_ack && protocol.hello_exchanged() {
        let goodbye = protocol.initiate_goodbye("client-initiated-close");
        info!(kind = goodbye.kind(), "transport smoke client sending control message");
        send_message(&mut send, &goodbye).await?;
        send.finish()?;
        info!("transport smoke client finished send side");
    }

    // Wait for server goodbye or stream close if server-initiated
    if !send_goodbye_after_ack {
        loop {
            match read_messages(&mut recv, &mut accumulator).await {
                Ok(messages) if messages.is_empty() => {
                    info!("transport smoke client control stream read finished (peer closed)");
                    break;
                }
                Ok(messages) => {
                    for message in messages {
                        info!(kind = message.kind(), "transport smoke client received control message");
                        protocol.on_receive(message)?;
                    }
                    if protocol.orderly_shutdown_complete() {
                        info!("transport smoke client orderly shutdown complete");
                        send.finish()?;
                        break;
                    }
                }
                Err(TransportError::ReadError(quinn::ReadExactError::FinishedEarly(_))) => {
                    info!("transport smoke client control stream finished");
                    send.finish()?;
                    break;
                }
                Err(e) => return Err(e),
            }
        }
    }

    info!(
        handshake_complete = protocol.handshake_complete(),
        orderly_shutdown = protocol.orderly_shutdown_complete(),
        "transport smoke client session complete"
    );
    Ok(())
}

async fn send_message(send: &mut SendStream, message: &ControlMessage) -> Result<(), TransportError> {
    let encoded = ControlMessageCodec::encode(message)?;
    send.write_all(&encoded).await.map_err(TransportError::WriteError)?;
    Ok(())
}

async fn read_messages(
    recv: &mut RecvStream,
    accumulator: &mut FrameAccumulator,
) -> Result<Vec<ControlMessage>, TransportError> {
    let mut buf = vec![0u8; 4096];
    match recv.read(&mut buf).await {
        Ok(Some(n)) => {
            accumulator.push(&buf[..n]);
            Ok(accumulator.drain_messages()?)
        }
        Ok(None) => Ok(Vec::new()),
        Err(e) => Err(TransportError::ReadError(quinn::ReadExactError::ReadError(e))),
    }
}

async fn await_with_optional_timeout<T>(
    duration: Option<Duration>,
    future: impl std::future::Future<Output = T>,
    timeout_reason: &'static str,
) -> Result<T, TransportError> {
    match duration {
        Some(duration) => timeout(duration, future)
            .await
            .map_err(|_| TransportError::Timeout(timeout_reason)),
        None => Ok(future.await),
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tls(error) => write!(formatter, "{error}"),
            Self::Quinn(error) => write!(formatter, "QUIC error: {error}"),
            Self::Connect(error) => write!(formatter, "connect error: {error}"),
            Self::WriteError(error) => write!(formatter, "write error: {error}"),
            Self::ReadError(error) => write!(formatter, "read error: {error}"),
            Self::ClosedStream(error) => write!(formatter, "closed stream: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
            Self::Connection(error) => write!(formatter, "connection error: {error}"),
            Self::InvalidEndpoint(endpoint) => write!(formatter, "invalid endpoint: {endpoint}"),
            Self::Timeout(reason) => write!(formatter, "{reason}"),
            Self::Runtime(reason) => write!(formatter, "{reason}"),
            Self::Auth(error) => write!(formatter, "auth error: {error}"),
        }
    }
}

impl Error for TransportError {}

impl From<TlsConfigError> for TransportError {
    fn from(value: TlsConfigError) -> Self {
        Self::Tls(value)
    }
}

impl From<quinn::ConnectError> for TransportError {
    fn from(value: quinn::ConnectError) -> Self {
        Self::Connect(value)
    }
}

impl From<quinn::ClosedStream> for TransportError {
    fn from(value: quinn::ClosedStream) -> Self {
        Self::ClosedStream(value)
    }
}

impl From<std::io::Error> for TransportError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ProtocolError> for TransportError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<ConnectionError> for TransportError {
    fn from(value: ConnectionError) -> Self {
        Self::Connection(value)
    }
}

impl From<AuthError> for TransportError {
    fn from(value: AuthError) -> Self {
        Self::Auth(value)
    }
}
