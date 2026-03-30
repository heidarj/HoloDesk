use std::{
    error::Error,
    ffi::c_void,
    fmt,
    net::{SocketAddr, ToSocketAddrs},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use msquic::{
    Addr, BufferRef, Configuration, Connection, ConnectionEvent, ConnectionRef,
    ConnectionShutdownFlags, CredentialConfig, ExecutionProfile, Listener, ListenerEvent,
    Registration, RegistrationConfig, SendFlags, Settings, Status, StatusCode, Stream,
    StreamEvent, StreamOpenFlags, StreamShutdownFlags, StreamStartFlags,
};
use tracing::{error, info, warn};

use crate::{
    connection::{ConnectionError, ConnectionRole, ControlConnection},
    config::{TransportClientConfig, TransportServerConfig},
    protocol::{ControlMessage, ControlMessageCodec, FrameAccumulator, ProtocolError},
    tls::{
        build_client_credential_config, build_server_credential_config, ClientValidationBinding,
        MsQuicCredentialBinding, TlsConfigError,
    },
};

#[derive(Debug)]
pub enum TransportError {
    Tls(TlsConfigError),
    MsQuic(Status),
    Io(std::io::Error),
    Protocol(ProtocolError),
    Connection(ConnectionError),
    InvalidEndpoint(String),
    Timeout(&'static str),
    Runtime(String),
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

#[derive(Debug, Clone)]
pub struct TransportServer {
    config: TransportServerConfig,
}

#[derive(Debug, Clone)]
pub struct TransportSmokeClient {
    config: TransportClientConfig,
}

const APP_NAME: &str = "holobridge-transport";
const SERVER_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const CLIENT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const CONTROL_STREAM_CLOSE_CODE: u64 = 0;

struct AlpnBuffer {
    bytes: Vec<u8>,
}

impl AlpnBuffer {
    fn new(alpn: &str) -> Self {
        Self {
            bytes: alpn.as_bytes().to_vec(),
        }
    }

    fn as_buffer_ref(&self) -> BufferRef {
        BufferRef::from(self.bytes.as_slice())
    }
}

struct PendingSend {
    buffer: Box<[u8]>,
}

#[derive(Default)]
struct CompletionState {
    result: Option<Result<(), TransportError>>,
}

#[derive(Default)]
struct CompletionSignal {
    state: Mutex<CompletionState>,
    condvar: Condvar,
}

impl CompletionSignal {
    fn resolve(&self, result: Result<(), TransportError>) {
        let mut state = self.state.lock().expect("completion signal poisoned");
        if state.result.is_none() {
            state.result = Some(result);
            self.condvar.notify_all();
        }
    }

    fn wait(&self, timeout: Duration, timeout_reason: &'static str) -> Result<(), TransportError> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().expect("completion signal poisoned");

        loop {
            if let Some(result) = state.result.take() {
                return result;
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(TransportError::Timeout(timeout_reason));
            }

            let remaining = deadline.saturating_duration_since(now);
            let (next_state, wait_result) = self
                .condvar
                .wait_timeout(state, remaining)
                .expect("completion signal poisoned while waiting");
            state = next_state;

            if wait_result.timed_out() && state.result.is_none() {
                return Err(TransportError::Timeout(timeout_reason));
            }
        }
    }
}

struct ControlStreamState {
    stream: Arc<Stream>,
    protocol: ControlConnection,
    accumulator: FrameAccumulator,
    stream_shutdown_complete: bool,
    shutdown_connection_when_stream_complete: bool,
}

impl ControlStreamState {
    fn new(stream: Arc<Stream>, role: ConnectionRole) -> Self {
        Self {
            stream,
            protocol: ControlConnection::new(role),
            accumulator: FrameAccumulator::default(),
            stream_shutdown_complete: false,
            shutdown_connection_when_stream_complete: false,
        }
    }
}

#[derive(Default)]
struct ServerRuntimeState {
    connection: Option<Arc<Connection>>,
    control: Option<ControlStreamState>,
    connection_shutdown_complete: bool,
    connection_shutdown_started: bool,
}

struct ServerShared {
    completion: CompletionSignal,
    runtime: Mutex<ServerRuntimeState>,
    server_initiated_close_after_ack: bool,
}

impl ServerShared {
    fn new(server_initiated_close_after_ack: bool) -> Self {
        Self {
            completion: CompletionSignal::default(),
            runtime: Mutex::new(ServerRuntimeState::default()),
            server_initiated_close_after_ack,
        }
    }

    fn fail(&self, error: TransportError) {
        self.completion.resolve(Err(error));
    }

    fn maybe_finish_success(&self) {
        let runtime = self.runtime.lock().expect("server runtime poisoned");
        let Some(control) = runtime.control.as_ref() else {
            return;
        };

        if runtime.connection_shutdown_complete
            && control.stream_shutdown_complete
            && control.protocol.handshake_complete()
            && control.protocol.orderly_shutdown_complete()
        {
            self.completion.resolve(Ok(()));
        }
    }
}

#[derive(Default)]
struct ClientRuntimeState {
    connection: Option<Arc<Connection>>,
    control: Option<ControlStreamState>,
    connection_shutdown_complete: bool,
}

struct ClientShared {
    completion: CompletionSignal,
    runtime: Mutex<ClientRuntimeState>,
    send_goodbye_after_ack: bool,
}

impl ClientShared {
    fn new(send_goodbye_after_ack: bool) -> Self {
        Self {
            completion: CompletionSignal::default(),
            runtime: Mutex::new(ClientRuntimeState::default()),
            send_goodbye_after_ack,
        }
    }

    fn fail(&self, error: TransportError) {
        self.completion.resolve(Err(error));
    }

    fn maybe_finish_success(&self) {
        let runtime = self.runtime.lock().expect("client runtime poisoned");
        let Some(control) = runtime.control.as_ref() else {
            return;
        };

        if runtime.connection_shutdown_complete
            && control.stream_shutdown_complete
            && control.protocol.handshake_complete()
            && control.protocol.orderly_shutdown_complete()
        {
            self.completion.resolve(Ok(()));
        }
    }
}

enum RuntimeAction {
    Send {
        stream: Arc<Stream>,
        bytes: Vec<u8>,
        finish: bool,
    },
    ShutdownStream {
        stream: Arc<Stream>,
    },
    ShutdownConnection {
        connection: Arc<Connection>,
    },
}

impl TransportServer {
    pub fn new(config: TransportServerConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &TransportServerConfig {
        &self.config
    }

    pub fn runtime_summary(&self) -> Result<ServerRuntimeSummary, TransportError> {
        let binding = MsQuicCredentialBinding::from_server_config(&self.config)?;

        Ok(ServerRuntimeSummary {
            backend: "MsQuic",
            bind_endpoint: self.config.listen_endpoint(),
            alpn: self.config.alpn.clone(),
            certificate: binding.describe(),
            close_mode: if self.config.server_initiated_close_after_ack {
                "server-initiated"
            } else {
                "client-initiated"
            },
        })
    }

    pub fn serve_once(&self) -> Result<(), TransportError> {
        let _ = self.runtime_summary()?;

        let registration = Arc::new(build_registration()?);
        let settings = build_settings();
        let alpn = AlpnBuffer::new(&self.config.alpn);
        let server_credentials = build_server_credential_config(&self.config)?;
        let configuration = Arc::new(build_configuration(
            &registration,
            &alpn,
            &settings,
            &server_credentials,
        )?);
        let bind_address = resolve_socket_addr(&self.config.listen_endpoint())?;
        let bind_addr = Addr::from(bind_address);
        let shared = Arc::new(ServerShared::new(self.config.server_initiated_close_after_ack));
        let listener = build_server_listener(&registration, &configuration, &shared)?;

        let listener_alpn = [alpn.as_buffer_ref()];
        listener.start(&listener_alpn, Some(&bind_addr))?;

        info!(endpoint = %bind_address, alpn = %self.config.alpn, "host transport listener started");

        let wait_result = shared
            .completion
            .wait(SERVER_WAIT_TIMEOUT, "timed out waiting for host transport session completion");

        listener.stop();
        wait_result
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
            validation: ClientValidationBinding::from_client_config(&self.config).describe(),
            close_mode: if self.config.send_goodbye_after_ack {
                "client-initiated"
            } else {
                "server-initiated"
            },
        }
    }

    pub fn run(&self) -> Result<(), TransportError> {
        let _ = self.runtime_summary();

        let registration = Arc::new(build_registration()?);
        let settings = build_settings();
        let alpn = AlpnBuffer::new(&self.config.alpn);
        let client_credentials = build_client_credential_config(&self.config);
        let configuration = build_configuration(
            &registration,
            &alpn,
            &settings,
            &client_credentials,
        )?;
        let shared = Arc::new(ClientShared::new(self.config.send_goodbye_after_ack));
        let connection = Arc::new(build_client_connection(&registration, &shared)?);

        {
            let mut runtime = shared.runtime.lock().expect("client runtime poisoned");
            runtime.connection = Some(Arc::clone(&connection));
        }

        let server_name = self
            .config
            .server_name
            .clone()
            .unwrap_or_else(|| self.config.server_host.clone());
        connection.start(&configuration, &server_name, self.config.server_port)?;

        info!(endpoint = %self.config.remote_endpoint(), server_name = %server_name, alpn = %self.config.alpn, "transport smoke client started");

        shared
            .completion
            .wait(CLIENT_WAIT_TIMEOUT, "timed out waiting for smoke client session completion")
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tls(error) => write!(formatter, "{error}"),
            Self::MsQuic(error) => write!(formatter, "MsQuic error: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
            Self::Connection(error) => write!(formatter, "connection error: {error}"),
            Self::InvalidEndpoint(endpoint) => write!(formatter, "invalid endpoint: {endpoint}"),
            Self::Timeout(reason) => write!(formatter, "{reason}"),
            Self::Runtime(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl Error for TransportError {}

impl From<TlsConfigError> for TransportError {
    fn from(value: TlsConfigError) -> Self {
        Self::Tls(value)
    }
}

impl From<Status> for TransportError {
    fn from(value: Status) -> Self {
        Self::MsQuic(value)
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

fn build_registration() -> Result<Registration, TransportError> {
    let config = RegistrationConfig::new()
        .set_app_name(APP_NAME.to_owned())
        .set_execution_profile(ExecutionProfile::LowLatency);
    Ok(Registration::new(&config)?)
}

fn build_settings() -> Settings {
    Settings::new()
        .set_PeerBidiStreamCount(1)
        .set_PeerUnidiStreamCount(0)
}

fn build_configuration(
    registration: &Registration,
    alpn: &AlpnBuffer,
    settings: &Settings,
    credentials: &CredentialConfig,
) -> Result<Configuration, TransportError> {
    let configuration_alpn = [alpn.as_buffer_ref()];
    let configuration = Configuration::open(registration, &configuration_alpn, Some(settings))?;
    configuration.load_credential(credentials)?;
    Ok(configuration)
}

fn resolve_socket_addr(endpoint: &str) -> Result<SocketAddr, TransportError> {
    endpoint
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| TransportError::InvalidEndpoint(endpoint.to_owned()))
}

fn callback_failure<T>(shared: &ServerShared, error: TransportError) -> Result<T, Status> {
    error!(error = %error, "host transport callback failed");
    shared.fail(error);
    Err(Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR))
}

fn client_callback_failure<T>(shared: &ClientShared, error: TransportError) -> Result<T, Status> {
    error!(error = %error, "transport smoke client callback failed");
    shared.fail(error);
    Err(Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR))
}

fn build_server_listener(
    registration: &Registration,
    configuration: &Arc<Configuration>,
    shared: &Arc<ServerShared>,
) -> Result<Listener, TransportError> {
    let configuration = Arc::clone(configuration);
    let shared = Arc::clone(shared);

    Ok(Listener::open(registration, move |_listener, event| {
        match event {
            ListenerEvent::NewConnection { info, connection } => {
                if let Some(remote) = info.remote_address.as_socket() {
                    info!(remote = %remote, alpn = %String::from_utf8_lossy(info.negotiated_alpn), "host transport accepted connection");
                }

                let owned_connection = Arc::new(unsafe { Connection::from_raw(connection.as_raw()) });
                let callback_shared = Arc::clone(&shared);
                owned_connection.set_callback_handler(move |connection, event| {
                    on_server_connection_event(&callback_shared, connection, event)
                });

                {
                    let mut runtime = shared.runtime.lock().expect("server runtime poisoned");
                    if runtime.connection.is_some() {
                        return callback_failure(
                            &shared,
                            TransportError::Runtime(
                                "server runtime only supports one connection per serve_once".to_owned(),
                            ),
                        );
                    }
                    runtime.connection = Some(Arc::clone(&owned_connection));
                }

                owned_connection.set_configuration(&configuration).map_err(|error| {
                    error!(error = %error, "failed to apply server configuration during acceptance");
                    shared.fail(TransportError::from(error.clone()));
                    error
                })
            }
            ListenerEvent::StopComplete { app_close_in_progress } => {
                info!(app_close_in_progress, "host transport listener stopped");
                Ok(())
            }
        }
    })?)
}

fn build_client_connection(
    registration: &Registration,
    shared: &Arc<ClientShared>,
) -> Result<Connection, TransportError> {
    let shared = Arc::clone(shared);
    Ok(Connection::open(registration, move |connection, event| {
        on_client_connection_event(&shared, connection, event)
    })?)
}

fn on_server_connection_event(
    shared: &Arc<ServerShared>,
    connection: ConnectionRef,
    event: ConnectionEvent,
) -> Result<(), Status> {
    match event {
        ConnectionEvent::Connected {
            session_resumed,
            negotiated_alpn,
        } => {
            let local = connection
                .get_local_addr()
                .ok()
                .and_then(|addr| addr.as_socket())
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "unknown".to_owned());
            let remote = connection
                .get_remote_addr()
                .ok()
                .and_then(|addr| addr.as_socket())
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "unknown".to_owned());
            info!(local = %local, remote = %remote, session_resumed, alpn = %String::from_utf8_lossy(negotiated_alpn), "host transport connection established");
            Ok(())
        }
        ConnectionEvent::PeerStreamStarted { stream, .. } => {
            let stream = Arc::new(unsafe { Stream::from_raw(stream.as_raw()) });
            let callback_shared = Arc::clone(shared);
            stream.set_callback_handler(move |stream, event| on_server_stream_event(&callback_shared, stream, event));

            let mut runtime = shared.runtime.lock().expect("server runtime poisoned");
            if runtime.control.is_some() {
                return callback_failure(
                    shared,
                    TransportError::Runtime(
                        "server runtime only supports one control stream per connection".to_owned(),
                    ),
                );
            }
            runtime.control = Some(ControlStreamState::new(stream, ConnectionRole::Server));
            Ok(())
        }
        ConnectionEvent::ShutdownInitiatedByTransport { status, error_code } => {
            warn!(status = %status, error_code, "host transport shutdown initiated by transport");
            Ok(())
        }
        ConnectionEvent::ShutdownInitiatedByPeer { error_code } => {
            warn!(error_code, "host transport shutdown initiated by peer");
            Ok(())
        }
        ConnectionEvent::ShutdownComplete {
            handshake_completed,
            peer_acknowledged_shutdown,
            app_close_in_progress,
        } => {
            info!(handshake_completed, peer_acknowledged_shutdown, app_close_in_progress, "host transport connection shutdown complete");
            {
                let mut runtime = shared.runtime.lock().expect("server runtime poisoned");
                runtime.connection_shutdown_complete = true;
            }
            shared.maybe_finish_success();
            Ok(())
        }
        _ => Ok(()),
    }
}

fn on_server_stream_event(
    shared: &Arc<ServerShared>,
    stream: msquic::StreamRef,
    event: StreamEvent,
) -> Result<(), Status> {
    match event {
        StreamEvent::Receive {
            total_buffer_length,
            buffers,
            flags,
            ..
        } => {
            let received = buffers.iter().flat_map(|buffer| buffer.as_bytes()).copied().collect::<Vec<_>>();
            let received_len = received.len() as u64;
            stream.receive_complete(*total_buffer_length);

            let mut actions = Vec::new();
            {
                let mut runtime = shared.runtime.lock().expect("server runtime poisoned");
                let Some(control) = runtime.control.as_mut() else {
                    return callback_failure(
                        shared,
                        TransportError::Runtime("server receive arrived before control stream state existed".to_owned()),
                    );
                };

                control.accumulator.push(&received);
                let messages = control.accumulator.drain_messages().map_err(|error| {
                    shared.fail(TransportError::from(error.clone()));
                    Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR)
                })?;

                for message in messages {
                    let is_goodbye = matches!(message, ControlMessage::Goodbye { .. });
                    let responses = control.protocol.on_receive(message).map_err(|error| {
                        shared.fail(TransportError::from(error.clone()));
                        Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR)
                    })?;
                    for response in responses {
                        let encoded = ControlMessageCodec::encode(&response).map_err(|error| {
                            shared.fail(TransportError::from(error.clone()));
                            Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR)
                        })?;
                        actions.push(RuntimeAction::Send {
                            stream: Arc::clone(&control.stream),
                            bytes: encoded,
                            finish: false,
                        });
                    }

                    if control.protocol.handshake_complete() && shared.server_initiated_close_after_ack {
                        let goodbye = control.protocol.initiate_goodbye("server-initiated-close");
                        control.shutdown_connection_when_stream_complete = true;
                        let encoded = ControlMessageCodec::encode(&goodbye).map_err(|error| {
                            shared.fail(TransportError::from(error.clone()));
                            Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR)
                        })?;
                        actions.push(RuntimeAction::Send {
                            stream: Arc::clone(&control.stream),
                            bytes: encoded,
                            finish: true,
                        });
                    }

                    if is_goodbye {
                        control.shutdown_connection_when_stream_complete = true;
                        actions.push(RuntimeAction::ShutdownStream {
                            stream: Arc::clone(&control.stream),
                        });
                    }
                }

                if flags.contains(msquic::ReceiveFlags::FIN) && control.protocol.orderly_shutdown_complete() {
                    control.shutdown_connection_when_stream_complete = true;
                }
            }

            if received_len > 0 {
                info!(bytes = received_len, fin = flags.contains(msquic::ReceiveFlags::FIN), "host transport received control-stream data");
            }

            run_actions(&actions)?;
            Ok(())
        }
        StreamEvent::SendComplete {
            cancelled,
            client_context,
        } => {
            if !client_context.is_null() {
                let _ = unsafe { Box::from_raw(client_context as *mut PendingSend) };
            }
            info!(cancelled, "host transport control-stream send completed");
            Ok(())
        }
        StreamEvent::PeerSendShutdown => {
            info!("host transport peer closed control-stream send side");
            Ok(())
        }
        StreamEvent::PeerSendAborted { error_code } => callback_failure(
            shared,
            TransportError::Runtime(format!("peer aborted server control-stream send with error code {error_code}")),
        ),
        StreamEvent::PeerReceiveAborted { error_code } => callback_failure(
            shared,
            TransportError::Runtime(format!("peer aborted server control-stream receive with error code {error_code}")),
        ),
        StreamEvent::SendShutdownComplete { graceful } => {
            info!(graceful, "host transport control-stream send shutdown complete");
            Ok(())
        }
        StreamEvent::ShutdownComplete { .. } => {
            let action = {
                let mut runtime = shared.runtime.lock().expect("server runtime poisoned");
                let Some(control) = runtime.control.as_mut() else {
                    return Ok(());
                };

                control.stream_shutdown_complete = true;
                if control.shutdown_connection_when_stream_complete && !runtime.connection_shutdown_started {
                    runtime.connection_shutdown_started = true;
                    runtime
                        .connection
                        .as_ref()
                        .map(|connection| RuntimeAction::ShutdownConnection {
                            connection: Arc::clone(connection),
                        })
                } else {
                    None
                }
            };

            if let Some(action) = action {
                run_actions(&[action])?;
            }

            shared.maybe_finish_success();
            Ok(())
        }
        _ => Ok(()),
    }
}

fn on_client_connection_event(
    shared: &Arc<ClientShared>,
    connection: ConnectionRef,
    event: ConnectionEvent,
) -> Result<(), Status> {
    match event {
        ConnectionEvent::Connected {
            session_resumed,
            negotiated_alpn,
        } => {
            let local = connection
                .get_local_addr()
                .ok()
                .and_then(|addr| addr.as_socket())
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "unknown".to_owned());
            let remote = connection
                .get_remote_addr()
                .ok()
                .and_then(|addr| addr.as_socket())
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "unknown".to_owned());
            info!(local = %local, remote = %remote, session_resumed, alpn = %String::from_utf8_lossy(negotiated_alpn), "transport smoke client connected");

            if let Err(error) = open_client_control_stream(shared, &connection) {
                return client_callback_failure(shared, error);
            }

            Ok(())
        }
        ConnectionEvent::ShutdownInitiatedByTransport { status, error_code } => {
            warn!(status = %status, error_code, "transport smoke client shutdown initiated by transport");
            Ok(())
        }
        ConnectionEvent::ShutdownInitiatedByPeer { error_code } => {
            info!(error_code, "transport smoke client shutdown initiated by peer");
            Ok(())
        }
        ConnectionEvent::ShutdownComplete {
            handshake_completed,
            peer_acknowledged_shutdown,
            app_close_in_progress,
        } => {
            info!(handshake_completed, peer_acknowledged_shutdown, app_close_in_progress, "transport smoke client connection shutdown complete");
            {
                let mut runtime = shared.runtime.lock().expect("client runtime poisoned");
                runtime.connection_shutdown_complete = true;
            }
            shared.maybe_finish_success();
            Ok(())
        }
        _ => Ok(()),
    }
}

fn open_client_control_stream(
    shared: &Arc<ClientShared>,
    connection: &Connection,
) -> Result<(), TransportError> {
    let callback_shared = Arc::clone(shared);
    let stream = Arc::new(Stream::open(connection, StreamOpenFlags::NONE, move |stream, event| {
        on_client_stream_event(&callback_shared, stream, event)
    })?);

    {
        let mut runtime = shared.runtime.lock().expect("client runtime poisoned");
        if runtime.control.is_some() {
            return Err(TransportError::Runtime(
                "client runtime only supports one control stream".to_owned(),
            ));
        }
        runtime.control = Some(ControlStreamState::new(Arc::clone(&stream), ConnectionRole::Client));
    }

    stream.start(StreamStartFlags::IMMEDIATE)?;

    let hello = ControlMessage::hello_smoke();
    {
        let mut runtime = shared.runtime.lock().expect("client runtime poisoned");
        runtime
            .control
            .as_mut()
            .expect("client control stream just installed")
            .protocol
            .record_outbound(hello.clone());
    }

    Ok(run_actions(&[RuntimeAction::Send {
        stream,
        bytes: ControlMessageCodec::encode(&hello)?,
        finish: false,
    }])?)
}

fn on_client_stream_event(
    shared: &Arc<ClientShared>,
    stream: msquic::StreamRef,
    event: StreamEvent,
) -> Result<(), Status> {
    match event {
        StreamEvent::Receive {
            total_buffer_length,
            buffers,
            flags,
            ..
        } => {
            let received = buffers.iter().flat_map(|buffer| buffer.as_bytes()).copied().collect::<Vec<_>>();
            let received_len = received.len() as u64;
            stream.receive_complete(*total_buffer_length);

            let mut actions = Vec::new();
            {
                let mut runtime = shared.runtime.lock().expect("client runtime poisoned");
                let Some(control) = runtime.control.as_mut() else {
                    return client_callback_failure(
                        shared,
                        TransportError::Runtime("client receive arrived before control stream state existed".to_owned()),
                    );
                };

                control.accumulator.push(&received);
                let messages = control.accumulator.drain_messages().map_err(|error| {
                    shared.fail(TransportError::from(error.clone()));
                    Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR)
                })?;

                for message in messages {
                    let is_goodbye = matches!(message, ControlMessage::Goodbye { .. });
                    control.protocol.on_receive(message).map_err(|error| {
                        shared.fail(TransportError::from(error.clone()));
                        Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR)
                    })?;

                    if control.protocol.handshake_complete() && shared.send_goodbye_after_ack {
                        let goodbye = control.protocol.initiate_goodbye("client-initiated-close");
                        let encoded = ControlMessageCodec::encode(&goodbye).map_err(|error| {
                            shared.fail(TransportError::from(error.clone()));
                            Status::new(StatusCode::QUIC_STATUS_INTERNAL_ERROR)
                        })?;
                        actions.push(RuntimeAction::Send {
                            stream: Arc::clone(&control.stream),
                            bytes: encoded,
                            finish: true,
                        });
                    }

                    if is_goodbye {
                        actions.push(RuntimeAction::ShutdownStream {
                            stream: Arc::clone(&control.stream),
                        });
                    }
                }

                if flags.contains(msquic::ReceiveFlags::FIN) && control.protocol.orderly_shutdown_complete() {
                    actions.push(RuntimeAction::ShutdownStream {
                        stream: Arc::clone(&control.stream),
                    });
                }
            }

            if received_len > 0 {
                info!(bytes = received_len, fin = flags.contains(msquic::ReceiveFlags::FIN), "transport smoke client received control-stream data");
            }

            run_actions(&actions)?;
            Ok(())
        }
        StreamEvent::SendComplete {
            cancelled,
            client_context,
        } => {
            if !client_context.is_null() {
                let _ = unsafe { Box::from_raw(client_context as *mut PendingSend) };
            }
            info!(cancelled, "transport smoke client control-stream send completed");
            Ok(())
        }
        StreamEvent::PeerSendShutdown => {
            info!("transport smoke client peer closed control-stream send side");
            Ok(())
        }
        StreamEvent::PeerSendAborted { error_code } => client_callback_failure(
            shared,
            TransportError::Runtime(format!("peer aborted client control-stream send with error code {error_code}")),
        ),
        StreamEvent::PeerReceiveAborted { error_code } => client_callback_failure(
            shared,
            TransportError::Runtime(format!("peer aborted client control-stream receive with error code {error_code}")),
        ),
        StreamEvent::ShutdownComplete { .. } => {
            {
                let mut runtime = shared.runtime.lock().expect("client runtime poisoned");
                if let Some(control) = runtime.control.as_mut() {
                    control.stream_shutdown_complete = true;
                }
            }
            shared.maybe_finish_success();
            Ok(())
        }
        _ => Ok(()),
    }
}

fn run_actions(actions: &[RuntimeAction]) -> Result<(), Status> {
    for action in actions {
        match action {
            RuntimeAction::Send {
                stream,
                bytes,
                finish,
            } => send_frame(stream, bytes.clone(), *finish)?,
            RuntimeAction::ShutdownStream { stream } => {
                stream.shutdown(StreamShutdownFlags::GRACEFUL, CONTROL_STREAM_CLOSE_CODE)?;
            }
            RuntimeAction::ShutdownConnection { connection } => {
                connection.shutdown(ConnectionShutdownFlags::NONE, CONTROL_STREAM_CLOSE_CODE);
            }
        }
    }

    Ok(())
}

fn send_frame(stream: &Stream, bytes: Vec<u8>, finish: bool) -> Result<(), Status> {
    let pending = Box::new(PendingSend {
        buffer: bytes.into_boxed_slice(),
    });
    let pending_ptr = Box::into_raw(pending);
    let pending_ref = unsafe { &*pending_ptr };
    let buffer = [BufferRef::from(&pending_ref.buffer[..])];

    let mut flags = SendFlags::NONE;
    if finish {
        flags.insert(SendFlags::FIN);
    }

    let result = unsafe { stream.send(&buffer, flags, pending_ptr.cast::<c_void>()) };
    if let Err(error) = result {
        let _ = unsafe { Box::from_raw(pending_ptr) };
        return Err(error);
    }

    Ok(())
}