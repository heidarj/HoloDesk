use std::{
    error::Error,
    fmt,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use holobridge_capture::{
    CaptureBackend, CaptureConfig, CaptureError, CaptureSession, CaptureTarget, DisplayId,
    DxgiCaptureBackend,
};
use holobridge_encode::{
    recommended_bitrate_bps, EncodeError, EncodedAccessUnit, EncoderAbortHandle, MfH264Encoder,
    VideoEncoder, VideoEncoderConfig,
};
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use tokio::{task::JoinHandle, time::{sleep, timeout}};
use tracing::{info, warn};

use holobridge_auth::{
    AuthConfig, AuthError, AuthorizedUserStore, ResumeTokenService, TokenValidator,
};
use holobridge_session::{SessionError, SessionManager};

use crate::{
    config::{TransportClientConfig, TransportServerConfig, VideoStreamConfig},
    connection::{ConnectionError, ConnectionRole, ControlConnection, HandshakeAction},
    media::{
        negotiated_datagram_payload_limit, H264DatagramPacketizer, MediaDatagramError,
        VIDEO_DATAGRAM_CAPABILITY,
    },
    protocol::{
        ControlMessage, ControlMessageCodec, FrameAccumulator, ProtocolError,
        CONTROL_STREAM_CAPABILITY,
    },
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
    Session(SessionError),
    Capture(CaptureError),
    Encode(EncodeError),
    Media(MediaDatagramError),
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
    resume_tokens: Option<Arc<ResumeTokenService>>,
    session_manager: Option<Arc<SessionManager>>,
}

#[derive(Debug, Clone)]
pub struct TransportSmokeClient {
    config: TransportClientConfig,
}

const CLIENT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

struct ActiveVideoStream {
    stop_requested: Arc<AtomicBool>,
    abort_handle: Arc<std::sync::Mutex<Option<EncoderAbortHandle>>>,
    monitor_handle: JoinHandle<()>,
}

impl TransportServer {
    pub fn new(config: TransportServerConfig) -> Self {
        Self {
            config,
            auth_validator: None,
            user_store: None,
            resume_tokens: None,
            session_manager: None,
        }
    }

    pub async fn with_auth(
        config: TransportServerConfig,
        auth_config: &AuthConfig,
    ) -> Result<Self, TransportError> {
        let validator = TokenValidator::new(auth_config)
            .await
            .map_err(TransportError::Auth)?;
        let user_store =
            AuthorizedUserStore::load(&auth_config.user_store_path, auth_config.bootstrap_mode)
                .await
                .map_err(TransportError::Auth)?;
        let resume_tokens = ResumeTokenService::new(auth_config).map_err(TransportError::Auth)?;
        let session_manager =
            SessionManager::new(resume_tokens.clone(), auth_config.resume_token_ttl_secs)
                .map_err(TransportError::Session)?;

        Ok(Self {
            config,
            auth_validator: Some(Arc::new(validator)),
            user_store: Some(Arc::new(user_store)),
            resume_tokens: Some(Arc::new(resume_tokens)),
            session_manager: Some(Arc::new(session_manager)),
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

    pub async fn serve(&self) -> Result<(), TransportError> {
        self.serve_internal(None).await
    }

    pub async fn serve_once(&self) -> Result<(), TransportError> {
        self.serve_internal(Some(1)).await
    }

    pub async fn serve_n(&self, max_connections: usize) -> Result<(), TransportError> {
        self.serve_internal(Some(max_connections)).await
    }

    async fn serve_internal(&self, max_connections: Option<usize>) -> Result<(), TransportError> {
        let server_config = build_server_config(&self.config)?;
        let bind_addr: SocketAddr = self
            .config
            .listen_endpoint()
            .parse()
            .map_err(|_| TransportError::InvalidEndpoint(self.config.listen_endpoint()))?;
        let endpoint = Endpoint::server(server_config, bind_addr)?;

        info!(endpoint = %bind_addr, alpn = %self.config.alpn, "host transport listener started");

        let mut handled_connections = 0usize;
        loop {
            if let Some(max_connections) = max_connections {
                if handled_connections >= max_connections {
                    break;
                }
            }

            let incoming = await_with_optional_timeout(
                self.config.server_wait_timeout,
                endpoint.accept(),
                "timed out waiting for incoming connection",
            )
            .await?
            .ok_or_else(|| {
                TransportError::Runtime("endpoint closed before accepting".to_owned())
            })?;

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
                connection.clone(),
                send,
                recv,
                self.config.server_initiated_close_after_ack,
                self.config.video.clone(),
                self.auth_validator.clone(),
                self.user_store.clone(),
                self.resume_tokens.clone(),
                self.session_manager.clone(),
            )
            .await;

            connection.close(quinn::VarInt::from_u32(0), b"done");
            handled_connections += 1;
            result?;
        }

        endpoint.close(quinn::VarInt::from_u32(0), b"done");
        endpoint.wait_idle().await;
        Ok(())
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
            validation: if self
                .config
                .debug_validation
                .allow_insecure_certificate_validation
            {
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
            self.config.request_video_stream,
            self.config.identity_token.as_deref(),
            self.config.resume_token.as_deref(),
        )
        .await;

        connection.close(quinn::VarInt::from_u32(0), b"done");
        endpoint.wait_idle().await;

        result
    }
}

impl ActiveVideoStream {
    fn stop(&self) {
        self.stop_requested.store(true, Ordering::Relaxed);
        // Try to unstick a blocked hardware encoder by sending MFT flush +
        // shutdown messages.  This is safe to call from any thread because
        // IMFTransform is free-threaded.
        let abort_handle = self.abort_handle.lock().unwrap().clone();
        if let Some(handle) = abort_handle {
            info!("host video: sending MFT abort (FLUSH + END_STREAMING)");
            let _ = std::thread::Builder::new()
                .name("holobridge-mft-abort".to_owned())
                .spawn(move || {
                    handle.abort();
                });
        }
    }

    fn detach(self) {
        tokio::spawn(async move {
            self.join().await;
        });
    }

    async fn join(self) {
        // Give the worker thread up to 30 seconds to stop.  The MFT abort
        // should unstick most ProcessInput/ProcessOutput blocks; after that
        // Windows TDR (typically 2 s) should reset the GPU and unblock
        // VideoProcessorBlt.  We keep the timeout generous so the
        // DuplicateOutput retry on the next session has the best chance
        // of succeeding.
        match timeout(Duration::from_secs(30), self.monitor_handle).await {
            Ok(_) => {
                info!("host video: worker joined successfully");
            }
            Err(_) => {
                warn!(
                    "host video: worker did not stop within 30 s — abandoning.  \
                     The next DuplicateOutput call will retry for up to 30 s."
                );
            }
        }
    }
}

fn start_video_stream(
    connection: Connection,
    video_config: VideoStreamConfig,
) -> Result<ActiveVideoStream, TransportError> {
    let initial_payload_limit = negotiated_datagram_payload_limit(
        connection.max_datagram_size(),
        video_config.datagram_payload_cap_bytes,
    )
    .ok_or_else(|| {
        TransportError::Runtime(
            "video datagrams are not available on this QUIC connection".to_owned(),
        )
    })?;
    info!(
        payload_limit = initial_payload_limit,
        "host video stream worker starting"
    );

    let stop_requested = Arc::new(AtomicBool::new(false));
    let abort_handle: Arc<std::sync::Mutex<Option<EncoderAbortHandle>>> =
        Arc::new(std::sync::Mutex::new(None));

    let stop_for_worker = Arc::clone(&stop_requested);
    let stop_for_monitor = Arc::clone(&stop_requested);
    let abort_for_worker = Arc::clone(&abort_handle);
    let connection_for_worker = connection.clone();
    let connection_for_monitor = connection.clone();
    let worker = tokio::task::spawn_blocking(move || {
        run_video_stream_worker(
            connection_for_worker,
            video_config,
            stop_for_worker,
            abort_for_worker,
        )
    });
    let monitor_handle = tokio::spawn(async move {
        match worker.await {
            Ok(Ok(())) => {
                info!("host video stream worker stopped gracefully");
            }
            Ok(Err(error)) => {
                if !stop_for_monitor.load(Ordering::Relaxed) {
                    warn!(error = %error, "host video stream worker failed — closing connection");
                    connection_for_monitor.close(
                        quinn::VarInt::from_u32(1),
                        b"video-stream-worker-failed",
                    );
                } else {
                    info!(error = %error, "host video stream worker ended after stop");
                }
            }
            Err(error) => {
                if !stop_for_monitor.load(Ordering::Relaxed) {
                    warn!(error = %error, "host video stream worker panicked — closing connection");
                    connection_for_monitor.close(
                        quinn::VarInt::from_u32(1),
                        b"video-stream-worker-panicked",
                    );
                }
            }
        }
    });

    Ok(ActiveVideoStream {
        stop_requested,
        abort_handle,
        monitor_handle,
    })
}

fn run_video_stream_worker(
    connection: Connection,
    video_config: VideoStreamConfig,
    stop_requested: Arc<AtomicBool>,
    abort_slot: Arc<std::sync::Mutex<Option<EncoderAbortHandle>>>,
) -> Result<(), TransportError> {
    if let Some(synthetic_access_units) = video_config.resolved_synthetic_access_units() {
        return run_synthetic_video_stream_worker(
            connection,
            &video_config,
            stop_requested,
            &synthetic_access_units,
        );
    }

    let backend = DxgiCaptureBackend::new().map_err(TransportError::Capture)?;
    let capture_target = match video_config.display_id.as_deref() {
        Some(display_id) => CaptureTarget::Display(
            display_id
                .parse::<DisplayId>()
                .map_err(|error| TransportError::Runtime(format!("invalid video display id: {error}")))?,
        ),
        None => CaptureTarget::Primary,
    };
    let mut capture = backend
        .open(
            capture_target,
            CaptureConfig {
                timeout_ms: video_config.capture_timeout_ms,
                target_fps_hint: Some(video_config.frame_rate_num),
            },
        )
        .map_err(TransportError::Capture)?;

    let first_frame = wait_for_first_frame(
        capture.as_mut(),
        Duration::from_secs(video_config.first_frame_timeout_secs),
        &stop_requested,
    )?;
    let metadata = first_frame.metadata();
    let bitrate_bps = video_config.bitrate_bps.unwrap_or_else(|| {
        recommended_bitrate_bps(
            metadata.width,
            metadata.height,
            video_config.frame_rate_num,
            video_config.frame_rate_den,
        )
    });
    let encoder_config = VideoEncoderConfig::new(
        metadata.width,
        metadata.height,
        bitrate_bps,
        video_config.frame_rate_num,
        video_config.frame_rate_den,
    );

    #[cfg(windows)]
    let mut encoder = MfH264Encoder::new(&capture.d3d11_device(), encoder_config)
        .map_err(TransportError::Encode)?;
    #[cfg(not(windows))]
    let mut encoder =
        MfH264Encoder::new(encoder_config).map_err(TransportError::Encode)?;

    // Publish the abort handle so the server can flush the MFT from outside
    // if the worker gets stuck in a blocking GPU/MFT call.
    *abort_slot.lock().unwrap() = encoder.abort_handle();

    let mut packetizer = H264DatagramPacketizer::default();
    let mut frames_sent: u64 = 0;
    let mut last_frame_time = Instant::now();
    let mut last_recoveries: u32 = 0;
    let mut last_loop_log = Instant::now();
    send_encoded_access_units(
        &connection,
        &video_config,
        &mut packetizer,
        encoder.encode(&first_frame).map_err(TransportError::Encode)?,
    )?;
    frames_sent += 1;
    info!(frames_sent, "host video: first frame sent");

    while !stop_requested.load(Ordering::Relaxed) {
        // Periodic heartbeat so we can tell the worker thread is alive
        if last_loop_log.elapsed() >= Duration::from_secs(2) {
            info!(
                frames_sent,
                since_last_frame_ms = last_frame_time.elapsed().as_millis() as u64,
                access_lost_recoveries = capture.access_lost_recoveries(),
                "host video: worker heartbeat"
            );
            last_loop_log = Instant::now();
        }

        let frame = match capture.acquire_frame() {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                let recoveries = capture.access_lost_recoveries();
                if recoveries != last_recoveries {
                    info!(
                        recoveries,
                        "host video: DXGI access-lost recovered, waiting for frames"
                    );
                    last_recoveries = recoveries;
                }
                continue;
            }
            Err(error) => {
                warn!(error = %error, "host video: acquire_frame error");
                return Err(TransportError::Capture(error));
            }
        };

        last_frame_time = Instant::now();
        let recoveries = capture.access_lost_recoveries();
        if recoveries != last_recoveries {
            info!(
                recoveries,
                frames_sent,
                "host video: resumed after access-lost recovery"
            );
            last_recoveries = recoveries;
        }

        // Bail immediately if the D3D device is in a bad state, rather than
        // blocking forever in a GPU call.
        if let Err(reason) = capture.check_device_health() {
            warn!(reason = %reason, frames_sent, "host video: D3D device unhealthy");
            return Err(TransportError::Runtime(reason));
        }

        let encode_start = Instant::now();
        let access_units = match encoder.encode(&frame) {
            Ok(aus) => aus,
            Err(error) => {
                let encode_ms = encode_start.elapsed().as_millis();
                warn!(error = %error, frames_sent, encode_ms, "host video: encode error");
                return Err(TransportError::Encode(error));
            }
        };
        let encode_ms = encode_start.elapsed().as_millis();
        if encode_ms > 50 {
            warn!(encode_ms, frames_sent, "host video: slow encode");
        }

        if let Err(error) = send_encoded_access_units(
            &connection,
            &video_config,
            &mut packetizer,
            access_units,
        ) {
            warn!(error = %error, frames_sent, "host video: send error");
            return Err(error);
        }

        frames_sent += 1;
        if frames_sent % 60 == 0 {
            info!(frames_sent, "host video: streaming");
        }
    }

    info!(
        frames_sent,
        access_lost_recoveries = capture.access_lost_recoveries(),
        "host video: worker stopped normally"
    );
    Ok(())
}

fn run_synthetic_video_stream_worker(
    connection: Connection,
    video_config: &VideoStreamConfig,
    stop_requested: Arc<AtomicBool>,
    synthetic_access_units: &[crate::config::SyntheticAccessUnit],
) -> Result<(), TransportError> {
    let frame_period = if video_config.frame_rate_num == 0 || video_config.frame_rate_den == 0 {
        Duration::from_millis(16)
    } else {
        Duration::from_secs_f64(
            video_config.frame_rate_den as f64 / video_config.frame_rate_num as f64,
        )
    };
    let mut packetizer = H264DatagramPacketizer::default();

    while !stop_requested.load(Ordering::Relaxed) {
        for access_unit in synthetic_access_units {
            if stop_requested.load(Ordering::Relaxed) {
                return Ok(());
            }

            let access_unit = EncodedAccessUnit {
                data: access_unit.data.clone(),
                is_keyframe: access_unit.is_keyframe,
                pts_100ns: access_unit.pts_100ns,
                duration_100ns: access_unit.duration_100ns,
            };
            send_encoded_access_units(
                &connection,
                video_config,
                &mut packetizer,
                vec![access_unit],
            )?;
            std::thread::sleep(frame_period);
        }
    }

    Ok(())
}

fn send_encoded_access_units(
    connection: &Connection,
    video_config: &VideoStreamConfig,
    packetizer: &mut H264DatagramPacketizer,
    access_units: Vec<EncodedAccessUnit>,
) -> Result<(), TransportError> {
    for access_unit in access_units {
        let payload_limit = match negotiated_datagram_payload_limit(
            connection.max_datagram_size(),
            video_config.datagram_payload_cap_bytes,
        ) {
            Some(limit) => limit,
            None => {
                // Transient: peer max datagram size temporarily unavailable or too
                // small.  Drop this frame rather than killing the video worker.
                warn!(
                    pts_100ns = access_unit.pts_100ns,
                    max_dg = ?connection.max_datagram_size(),
                    "dropping frame: datagram payload limit unavailable"
                );
                continue;
            }
        };

        let datagrams = packetizer
            .packetize(&access_unit, payload_limit)
            .map_err(TransportError::Media)?;
        for datagram in datagrams {
            match connection.send_datagram(datagram) {
                Ok(()) => {}
                Err(quinn::SendDatagramError::UnsupportedByPeer) => {
                    return Err(TransportError::Runtime(
                        "peer does not support QUIC datagrams".to_owned(),
                    ))
                }
                Err(quinn::SendDatagramError::Disabled) => {
                    return Err(TransportError::Runtime(
                        "QUIC datagram support is disabled locally".to_owned(),
                    ))
                }
                Err(quinn::SendDatagramError::TooLarge) => {
                    warn!(
                        pts_100ns = access_unit.pts_100ns,
                        "dropping oversized access unit after MTU change"
                    );
                    break;
                }
                Err(quinn::SendDatagramError::ConnectionLost(error)) => {
                    warn!(error = %error, "video datagram send: connection lost");
                    return Err(TransportError::Quinn(error))
                }
            }
        }
    }

    Ok(())
}

fn wait_for_first_frame(
    capture: &mut dyn CaptureSession,
    timeout: Duration,
    stop_requested: &AtomicBool,
) -> Result<holobridge_capture::CapturedFrame, TransportError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if stop_requested.load(Ordering::Relaxed) {
            return Err(TransportError::Runtime(
                "video stream startup was cancelled".to_owned(),
            ));
        }
        if let Some(frame) = capture.acquire_frame().map_err(TransportError::Capture)? {
            return Ok(frame);
        }
    }

    Err(TransportError::Timeout(
        "timed out waiting for the first captured video frame",
    ))
}

async fn run_server_control_stream(
    connection: Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    server_initiated_close: bool,
    video_config: VideoStreamConfig,
    auth_validator: Option<Arc<TokenValidator>>,
    user_store: Option<Arc<AuthorizedUserStore>>,
    resume_tokens: Option<Arc<ResumeTokenService>>,
    session_manager: Option<Arc<SessionManager>>,
) -> Result<(), TransportError> {
    let mut protocol = ControlConnection::new(ConnectionRole::Server);
    let mut accumulator = FrameAccumulator::default();
    let mut active_session_id: Option<String> = None;
    let mut session_established = false;
    let mut client_capabilities = Vec::new();
    let mut video_stream: Option<ActiveVideoStream> = None;

    let messages = read_messages(&mut recv, &mut accumulator).await?;
    for message in messages {
        if let ControlMessage::Hello { capabilities, .. } = &message {
            client_capabilities = capabilities.clone();
        }
        info!(
            kind = message.kind(),
            "host transport received control message"
        );
        let (responses, _handshake_action) = protocol.on_receive(message)?;
        for response in &responses {
            info!(
                kind = response.kind(),
                "host transport sending control message"
            );
            send_message(&mut send, response).await?;
        }
    }

    if auth_validator.is_some() {
        info!("host transport waiting for session handshake");
        let handshake_messages = read_messages(&mut recv, &mut accumulator).await?;
        for message in handshake_messages {
            info!(
                kind = message.kind(),
                "host transport received control message"
            );
            let (_responses, handshake_action) = protocol.on_receive(message)?;

            match handshake_action {
                Some(HandshakeAction::ValidateToken(token)) => {
                    let validator = auth_validator.as_ref().unwrap();
                    let store = user_store.as_ref().unwrap();
                    let sessions = session_manager.as_ref().unwrap();

                    match validator.validate(&token).await {
                        Ok(claims) => {
                            let sub = &claims.sub;
                            let authorized = store
                                .check_or_bootstrap(sub, claims.email.as_deref())
                                .await
                                .map_err(TransportError::Auth)?;

                            if authorized {
                                let created = sessions
                                    .create_session(sub, claims.email.clone())
                                    .await
                                    .map_err(TransportError::Session)?;
                                info!(sub, session_id = %created.session_id, "auth succeeded");
                                active_session_id = Some(created.session_id.clone());
                                session_established = true;
                                let result = protocol.record_auth_result(
                                    true,
                                    "authenticated",
                                    claims.email.clone(),
                                    Some(created.session_id),
                                    Some(created.resume_token),
                                    Some(created.resume_token_ttl_secs),
                                );
                                send_message(&mut send, &result).await?;
                                if video_config.enabled
                                    && client_capabilities.iter().any(|capability| {
                                        capability == VIDEO_DATAGRAM_CAPABILITY
                                    })
                                {
                                    video_stream = Some(start_video_stream(
                                        connection.clone(),
                                        video_config.clone(),
                                    )?);
                                }
                            } else {
                                warn!(sub, "auth failed: user not authorized");
                                let result = protocol.record_auth_result(
                                    false,
                                    "user not authorized",
                                    None,
                                    None,
                                    None,
                                    None,
                                );
                                send_message(&mut send, &result).await?;
                                send.finish()?;
                                sleep(Duration::from_millis(50)).await;
                                return Ok(());
                            }
                        }
                        Err(error) => {
                            warn!(error = %error, "auth failed: token validation error");
                            let result = protocol.record_auth_result(
                                false,
                                error.to_string(),
                                None,
                                None,
                                None,
                                None,
                            );
                            send_message(&mut send, &result).await?;
                            send.finish()?;
                            sleep(Duration::from_millis(50)).await;
                            return Ok(());
                        }
                    }
                }
                Some(HandshakeAction::ValidateResumeToken(token)) => {
                    let resume_tokens = resume_tokens.as_ref().unwrap();
                    let sessions = session_manager.as_ref().unwrap();

                    match resume_tokens.validate(&token) {
                        Ok(claims) => match sessions.resume_session(&claims).await {
                            Ok(resumed) => {
                                info!(
                                    session_id = %resumed.session_id,
                                    reconnect_count = resumed.reconnect_count,
                                    "session resume succeeded"
                                );
                                active_session_id = Some(resumed.session_id.clone());
                                session_established = true;
                                let result = protocol.record_resume_result(
                                    true,
                                    "resumed",
                                    resumed.user_display_name.clone(),
                                    Some(resumed.session_id),
                                    Some(resumed.resume_token),
                                    Some(resumed.resume_token_ttl_secs),
                                );
                                send_message(&mut send, &result).await?;
                                if video_config.enabled
                                    && client_capabilities.iter().any(|capability| {
                                        capability == VIDEO_DATAGRAM_CAPABILITY
                                    })
                                {
                                    video_stream = Some(start_video_stream(
                                        connection.clone(),
                                        video_config.clone(),
                                    )?);
                                }
                            }
                            Err(error) => {
                                warn!(error = %error, "session resume failed");
                                let result = protocol.record_resume_result(
                                    false,
                                    error.to_string(),
                                    None,
                                    None,
                                    None,
                                    None,
                                );
                                send_message(&mut send, &result).await?;
                                send.finish()?;
                                sleep(Duration::from_millis(50)).await;
                                return Ok(());
                            }
                        },
                        Err(error) => {
                            warn!(error = %error, "resume token validation failed");
                            let result = protocol.record_resume_result(
                                false,
                                error.to_string(),
                                None,
                                None,
                                None,
                                None,
                            );
                            send_message(&mut send, &result).await?;
                            send.finish()?;
                            sleep(Duration::from_millis(50)).await;
                            return Ok(());
                        }
                    }
                }
                None => {}
            }
        }
    }

    if server_initiated_close && protocol.hello_exchanged() {
        let goodbye = protocol.initiate_goodbye("server-initiated-close");
        info!(
            kind = goodbye.kind(),
            "host transport sending control message"
        );
        send_message(&mut send, &goodbye).await?;
        send.finish()?;
        info!("host transport finished send side");
    }

    let mut unexpected_disconnect = false;
    loop {
        match read_messages(&mut recv, &mut accumulator).await {
            Ok(messages) if messages.is_empty() => {
                info!("host transport control stream read finished (peer closed)");
                unexpected_disconnect =
                    session_established && !protocol.orderly_shutdown_complete();
                break;
            }
            Ok(messages) => {
                for message in messages {
                    info!(
                        kind = message.kind(),
                        "host transport received control message"
                    );
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
                unexpected_disconnect =
                    session_established && !protocol.orderly_shutdown_complete();
                break;
            }
            Err(error) if session_established => {
                warn!(error = %error, "host transport connection ended unexpectedly after session establishment");
                unexpected_disconnect = true;
                break;
            }
            Err(error) => return Err(error),
        }
    }

    if !server_initiated_close {
        send.finish()?;
    }

    if let Some(session_id) = active_session_id {
        if let Some(sessions) = &session_manager {
            if unexpected_disconnect {
                sessions
                    .hold_session(&session_id)
                    .await
                    .map_err(TransportError::Session)?;
                info!(session_id, "host session moved to held state");
            } else {
                sessions
                    .terminate_session(&session_id, "control-stream-closed")
                    .await
                    .map_err(TransportError::Session)?;
                info!(session_id, "host session terminated");
            }
        }
    }

    if let Some(video_stream) = video_stream {
        video_stream.stop();
        info!(
            orderly_shutdown = protocol.orderly_shutdown_complete(),
            unexpected_disconnect,
            "host video: cleanup detached from control stream"
        );
        video_stream.detach();
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
    request_video_stream: bool,
    identity_token: Option<&str>,
    resume_token: Option<&str>,
) -> Result<(), TransportError> {
    let mut protocol = ControlConnection::new(ConnectionRole::Client);
    let mut accumulator = FrameAccumulator::default();

    let hello = hello_message(request_video_stream);
    protocol.record_outbound(hello.clone());
    info!(
        kind = hello.kind(),
        "transport smoke client sending control message"
    );
    send_message(&mut send, &hello).await?;

    let messages = read_messages(&mut recv, &mut accumulator).await?;
    for message in messages {
        info!(
            kind = message.kind(),
            "transport smoke client received control message"
        );
        protocol.on_receive(message)?;
    }

    if let Some(token) = resume_token {
        if protocol.hello_exchanged() {
            let resume = ControlMessage::resume_session(token);
            protocol.record_outbound(resume.clone());
            info!(
                kind = resume.kind(),
                "transport smoke client sending control message"
            );
            send_message(&mut send, &resume).await?;

            let resume_messages = read_messages(&mut recv, &mut accumulator).await?;
            for message in resume_messages {
                info!(
                    kind = message.kind(),
                    "transport smoke client received control message"
                );
                protocol.on_receive(message)?;
            }

            if !protocol.session_established() {
                info!("transport smoke client resume was rejected");
                send.finish()?;
                return Ok(());
            }
        }
    } else if let Some(token) = identity_token {
        if protocol.hello_exchanged() {
            let auth = ControlMessage::authenticate(token);
            protocol.record_outbound(auth.clone());
            info!(
                kind = auth.kind(),
                "transport smoke client sending control message"
            );
            send_message(&mut send, &auth).await?;

            let auth_messages = read_messages(&mut recv, &mut accumulator).await?;
            for message in auth_messages {
                info!(
                    kind = message.kind(),
                    "transport smoke client received control message"
                );
                protocol.on_receive(message)?;
            }

            if !protocol.session_established() {
                info!("transport smoke client auth was rejected");
                send.finish()?;
                return Ok(());
            }
        }
    }

    if send_goodbye_after_ack && protocol.hello_exchanged() {
        let goodbye = protocol.initiate_goodbye("client-initiated-close");
        info!(
            kind = goodbye.kind(),
            "transport smoke client sending control message"
        );
        send_message(&mut send, &goodbye).await?;
        send.finish()?;
        info!("transport smoke client finished send side");
    }

    if !send_goodbye_after_ack {
        loop {
            match read_messages(&mut recv, &mut accumulator).await {
                Ok(messages) if messages.is_empty() => {
                    info!("transport smoke client control stream read finished (peer closed)");
                    break;
                }
                Ok(messages) => {
                    for message in messages {
                        info!(
                            kind = message.kind(),
                            "transport smoke client received control message"
                        );
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
                Err(error) => return Err(error),
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

async fn send_message(
    send: &mut SendStream,
    message: &ControlMessage,
) -> Result<(), TransportError> {
    let encoded = ControlMessageCodec::encode(message)?;
    send.write_all(&encoded)
        .await
        .map_err(TransportError::WriteError)?;
    Ok(())
}

async fn read_messages(
    recv: &mut RecvStream,
    accumulator: &mut FrameAccumulator,
) -> Result<Vec<ControlMessage>, TransportError> {
    loop {
        let messages = accumulator.drain_messages()?;
        if !messages.is_empty() {
            return Ok(messages);
        }

        let mut buf = vec![0u8; 4096];
        match recv.read(&mut buf).await {
            Ok(Some(n)) => accumulator.push(&buf[..n]),
            Ok(None) => return Ok(Vec::new()),
            Err(error) => {
                return Err(TransportError::ReadError(quinn::ReadExactError::ReadError(
                    error,
                )))
            }
        }
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
            Self::Session(error) => write!(formatter, "session error: {error}"),
            Self::Capture(error) => write!(formatter, "capture error: {error}"),
            Self::Encode(error) => write!(formatter, "encode error: {error}"),
            Self::Media(error) => write!(formatter, "media datagram error: {error}"),
        }
    }
}

fn hello_message(request_video_stream: bool) -> ControlMessage {
    let mut capabilities = vec![CONTROL_STREAM_CAPABILITY.to_owned()];
    if request_video_stream {
        capabilities.push(VIDEO_DATAGRAM_CAPABILITY.to_owned());
    }
    ControlMessage::hello("transport-smoke", capabilities)
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

impl From<SessionError> for TransportError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

impl From<CaptureError> for TransportError {
    fn from(value: CaptureError) -> Self {
        Self::Capture(value)
    }
}

impl From<EncodeError> for TransportError {
    fn from(value: EncodeError) -> Self {
        Self::Encode(value)
    }
}

impl From<MediaDatagramError> for TransportError {
    fn from(value: MediaDatagramError) -> Self {
        Self::Media(value)
    }
}

#[cfg(test)]
mod tests {
    use std::{net::UdpSocket, path::PathBuf, time::Instant};

    use tempfile::TempDir;
    use tokio::time::sleep;

    use holobridge_auth::test_keys::{create_test_jwt, generate_test_rsa_keypair};

    use crate::{
        config::{SyntheticVideoPreset, VideoSource},
        media::{H264DatagramReassembler, ReassembledAccessUnit, ReassemblerConfig},
    };

    use super::*;

    enum ClientHandshake<'a> {
        Authenticate(&'a str),
        Resume(&'a str),
    }

    fn free_port() -> u16 {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind udp socket");
        socket.local_addr().expect("local addr").port()
    }

    fn test_auth_config(tmp: &TempDir, pub_key_path: &str, ttl_secs: u64) -> AuthConfig {
        AuthConfig {
            apple_bundle_id: "cloud.hr5.HoloBridge".to_owned(),
            jwks_cache_ttl_secs: 3600,
            user_store_path: tmp.path().join("users.json"),
            bootstrap_mode: true,
            test_mode: true,
            test_public_key_pem: Some(PathBuf::from(pub_key_path)),
            resume_token_ttl_secs: ttl_secs,
            resume_token_secret: Some("transport-test-resume-secret".to_owned()),
        }
    }

    fn test_server_config(port: u16) -> TransportServerConfig {
        let mut config = TransportServerConfig::default();
        config.bind_address = "127.0.0.1".to_owned();
        config.port = port;
        config.server_wait_timeout = Some(Duration::from_secs(5));
        config
    }

    fn test_client_config(port: u16) -> TransportClientConfig {
        let mut config = TransportClientConfig::default();
        config.server_host = "127.0.0.1".to_owned();
        config.server_port = port;
        config.server_name = Some("localhost".to_owned());
        config
            .debug_validation
            .allow_insecure_certificate_validation = true;
        config.send_goodbye_after_ack = true;
        config
    }

    async fn run_client_handshake(
        config: &TransportClientConfig,
        handshake: ClientHandshake<'_>,
        send_goodbye: bool,
    ) -> Result<ControlMessage, TransportError> {
        let client_config = build_client_config(config)?;
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
        endpoint.set_default_client_config(client_config);

        let server_addr: SocketAddr = config
            .remote_endpoint()
            .parse()
            .map_err(|_| TransportError::InvalidEndpoint(config.remote_endpoint()))?;
        let server_name = config
            .server_name
            .clone()
            .unwrap_or_else(|| config.server_host.clone());

        let connection = timeout(
            CLIENT_WAIT_TIMEOUT,
            endpoint.connect(server_addr, &server_name)?,
        )
        .await
        .map_err(|_| TransportError::Timeout("timed out connecting to server"))?
        .map_err(TransportError::Quinn)?;

        let (mut send, mut recv) = connection.open_bi().await.map_err(TransportError::Quinn)?;
        let mut accumulator = FrameAccumulator::default();

        send_message(&mut send, &hello_message(config.request_video_stream)).await?;
        let hello_messages = read_messages(&mut recv, &mut accumulator).await?;
        assert!(hello_messages
            .iter()
            .any(|message| matches!(message, ControlMessage::HelloAck { .. })));

        match handshake {
            ClientHandshake::Authenticate(token) => {
                send_message(&mut send, &ControlMessage::authenticate(token)).await?;
            }
            ClientHandshake::Resume(token) => {
                send_message(&mut send, &ControlMessage::resume_session(token)).await?;
            }
        }

        let result_messages = read_messages(&mut recv, &mut accumulator).await?;
        let result = result_messages
            .into_iter()
            .find(|message| {
                matches!(
                    message,
                    ControlMessage::AuthResult { .. } | ControlMessage::ResumeResult { .. }
                )
            })
            .expect("expected auth_result or resume_result");

        if send_goodbye {
            send_message(&mut send, &ControlMessage::goodbye("test-goodbye")).await?;
            send.finish()?;
        }

        connection.close(quinn::VarInt::from_u32(0), b"done");
        endpoint.wait_idle().await;
        Ok(result)
    }

    struct ConnectedClient {
        endpoint: Endpoint,
        connection: Connection,
        send: SendStream,
        recv: RecvStream,
        accumulator: FrameAccumulator,
    }

    impl ConnectedClient {
        async fn connect(config: &TransportClientConfig) -> Result<Self, TransportError> {
            let client_config = build_client_config(config)?;
            let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
            endpoint.set_default_client_config(client_config);

            let server_addr: SocketAddr = config
                .remote_endpoint()
                .parse()
                .map_err(|_| TransportError::InvalidEndpoint(config.remote_endpoint()))?;
            let server_name = config
                .server_name
                .clone()
                .unwrap_or_else(|| config.server_host.clone());

            let connection = timeout(
                CLIENT_WAIT_TIMEOUT,
                endpoint.connect(server_addr, &server_name)?,
            )
            .await
            .map_err(|_| TransportError::Timeout("timed out connecting to server"))?
            .map_err(TransportError::Quinn)?;

            let (send, recv) = connection.open_bi().await.map_err(TransportError::Quinn)?;
            Ok(Self {
                endpoint,
                connection,
                send,
                recv,
                accumulator: FrameAccumulator::default(),
            })
        }

        async fn exchange_hello(&mut self, request_video_stream: bool) -> Result<(), TransportError> {
            send_message(&mut self.send, &hello_message(request_video_stream)).await?;
            let hello_messages = read_messages(&mut self.recv, &mut self.accumulator).await?;
            assert!(hello_messages
                .iter()
                .any(|message| matches!(message, ControlMessage::HelloAck { .. })));
            Ok(())
        }

        async fn authenticate(&mut self, identity_token: &str) -> Result<ControlMessage, TransportError> {
            send_message(
                &mut self.send,
                &ControlMessage::authenticate(identity_token),
            )
            .await?;
            self.read_session_result().await
        }

        async fn resume(&mut self, resume_token: &str) -> Result<ControlMessage, TransportError> {
            send_message(
                &mut self.send,
                &ControlMessage::resume_session(resume_token),
            )
            .await?;
            self.read_session_result().await
        }

        async fn read_media_access_unit(
            &self,
            reassembler: &mut H264DatagramReassembler,
        ) -> Result<ReassembledAccessUnit, TransportError> {
            loop {
                let datagram = timeout(Duration::from_secs(2), self.connection.read_datagram())
                    .await
                    .map_err(|_| {
                        TransportError::Timeout(
                            "timed out waiting for media datagram during loopback test",
                        )
                    })?
                    .map_err(TransportError::Quinn)?;

                if let Some(access_unit) = reassembler
                    .push_datagram(&datagram, Instant::now())
                    .map_err(TransportError::Media)?
                {
                    return Ok(access_unit);
                }
            }
        }

        async fn close_gracefully(mut self) -> Result<(), TransportError> {
            send_message(&mut self.send, &ControlMessage::goodbye("test-goodbye")).await?;
            self.send.finish()?;
            self.connection.close(quinn::VarInt::from_u32(0), b"done");
            self.endpoint.wait_idle().await;
            Ok(())
        }

        async fn close_abrupt(self) {
            self.connection.close(quinn::VarInt::from_u32(0), b"done");
            self.endpoint.wait_idle().await;
        }

        async fn read_session_result(&mut self) -> Result<ControlMessage, TransportError> {
            let result_messages = read_messages(&mut self.recv, &mut self.accumulator).await?;
            Ok(result_messages
                .into_iter()
                .find(|message| {
                    matches!(
                        message,
                        ControlMessage::AuthResult { .. } | ControlMessage::ResumeResult { .. }
                    )
                })
                .expect("expected auth_result or resume_result"))
        }
    }

    fn test_video_payload() -> Vec<u8> {
        SyntheticVideoPreset::TransportLoopbackV1
            .build_access_units(60, 1)
            .into_iter()
            .next()
            .expect("synthetic access unit")
            .data
    }

    fn test_video_server_config(port: u16) -> TransportServerConfig {
        let mut config = test_server_config(port);
        config.video.enabled = true;
        config.video.source = VideoSource::SyntheticLoopback;
        config.video.synthetic_preset = Some(SyntheticVideoPreset::TransportLoopbackV1);
        config
    }

    #[tokio::test]
    async fn loopback_auth_drop_and_resume_succeeds() {
        let (private_pem, public_pem) = generate_test_rsa_keypair();
        let tmp = TempDir::new().unwrap();
        let pub_key_path = tmp.path().join("pub.pem");
        std::fs::write(&pub_key_path, &public_pem).unwrap();

        let port = free_port();
        let auth_config = test_auth_config(&tmp, pub_key_path.to_str().unwrap(), 60);
        let server = TransportServer::with_auth(test_server_config(port), &auth_config)
            .await
            .unwrap();

        let server_task = tokio::spawn(async move { server.serve_n(2).await });
        sleep(Duration::from_millis(100)).await;

        let identity_token = create_test_jwt(
            &private_pem,
            "resume-user-1",
            &auth_config.apple_bundle_id,
            false,
        );
        let client_config = test_client_config(port);

        let auth_result = run_client_handshake(
            &client_config,
            ClientHandshake::Authenticate(&identity_token),
            false,
        )
        .await
        .unwrap();

        let (session_id, resume_token) = match auth_result {
            ControlMessage::AuthResult {
                success,
                session_id,
                resume_token,
                ..
            } => {
                assert!(success);
                (session_id.unwrap(), resume_token.unwrap())
            }
            other => panic!("unexpected message: {:?}", other),
        };

        let resume_result =
            run_client_handshake(&client_config, ClientHandshake::Resume(&resume_token), true)
                .await
                .unwrap();

        match resume_result {
            ControlMessage::ResumeResult {
                success,
                session_id: resumed_session_id,
                resume_token: rotated_resume_token,
                ..
            } => {
                assert!(success);
                assert_eq!(resumed_session_id.as_deref(), Some(session_id.as_str()));
                assert_ne!(rotated_resume_token.as_deref(), Some(resume_token.as_str()));
            }
            other => panic!("unexpected message: {:?}", other),
        }

        server_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn loopback_reused_resume_token_is_rejected() {
        let (private_pem, public_pem) = generate_test_rsa_keypair();
        let tmp = TempDir::new().unwrap();
        let pub_key_path = tmp.path().join("pub.pem");
        std::fs::write(&pub_key_path, &public_pem).unwrap();

        let port = free_port();
        let auth_config = test_auth_config(&tmp, pub_key_path.to_str().unwrap(), 60);
        let server = TransportServer::with_auth(test_server_config(port), &auth_config)
            .await
            .unwrap();

        let server_task = tokio::spawn(async move { server.serve_n(3).await });
        sleep(Duration::from_millis(100)).await;

        let identity_token = create_test_jwt(
            &private_pem,
            "resume-user-2",
            &auth_config.apple_bundle_id,
            false,
        );
        let client_config = test_client_config(port);

        let auth_result = run_client_handshake(
            &client_config,
            ClientHandshake::Authenticate(&identity_token),
            false,
        )
        .await
        .unwrap();

        let initial_resume_token = match auth_result {
            ControlMessage::AuthResult {
                resume_token,
                success,
                ..
            } => {
                assert!(success);
                resume_token.unwrap()
            }
            other => panic!("unexpected message: {:?}", other),
        };

        let resume_result = run_client_handshake(
            &client_config,
            ClientHandshake::Resume(&initial_resume_token),
            false,
        )
        .await
        .unwrap();
        match resume_result {
            ControlMessage::ResumeResult { success, .. } => assert!(success),
            other => panic!("unexpected message: {:?}", other),
        }

        let rejected_result = run_client_handshake(
            &client_config,
            ClientHandshake::Resume(&initial_resume_token),
            true,
        )
        .await
        .unwrap();

        match rejected_result {
            ControlMessage::ResumeResult {
                success, message, ..
            } => {
                assert!(!success);
                assert!(message.contains("resume token") || message.contains("not resumable"));
            }
            other => panic!("unexpected message: {:?}", other),
        }

        server_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn loopback_expired_resume_token_is_rejected() {
        let (private_pem, public_pem) = generate_test_rsa_keypair();
        let tmp = TempDir::new().unwrap();
        let pub_key_path = tmp.path().join("pub.pem");
        std::fs::write(&pub_key_path, &public_pem).unwrap();

        let port = free_port();
        let auth_config = test_auth_config(&tmp, pub_key_path.to_str().unwrap(), 1);
        let server = TransportServer::with_auth(test_server_config(port), &auth_config)
            .await
            .unwrap();

        let server_task = tokio::spawn(async move { server.serve_n(2).await });
        sleep(Duration::from_millis(100)).await;

        let identity_token = create_test_jwt(
            &private_pem,
            "resume-user-3",
            &auth_config.apple_bundle_id,
            false,
        );
        let client_config = test_client_config(port);

        let auth_result = run_client_handshake(
            &client_config,
            ClientHandshake::Authenticate(&identity_token),
            false,
        )
        .await
        .unwrap();

        let initial_resume_token = match auth_result {
            ControlMessage::AuthResult {
                resume_token,
                success,
                ..
            } => {
                assert!(success);
                resume_token.unwrap()
            }
            other => panic!("unexpected message: {:?}", other),
        };

        sleep(Duration::from_secs(2)).await;

        let rejected_result = run_client_handshake(
            &client_config,
            ClientHandshake::Resume(&initial_resume_token),
            true,
        )
        .await
        .unwrap();

        match rejected_result {
            ControlMessage::ResumeResult {
                success, message, ..
            } => {
                assert!(!success);
                assert!(message.contains("expired"));
            }
            other => panic!("unexpected message: {:?}", other),
        }

        server_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn loopback_auth_starts_media_datagrams_and_reassembles_access_units() {
        let (private_pem, public_pem) = generate_test_rsa_keypair();
        let tmp = TempDir::new().unwrap();
        let pub_key_path = tmp.path().join("pub.pem");
        std::fs::write(&pub_key_path, &public_pem).unwrap();

        let port = free_port();
        let auth_config = test_auth_config(&tmp, pub_key_path.to_str().unwrap(), 60);
        let server = TransportServer::with_auth(test_video_server_config(port), &auth_config)
            .await
            .unwrap();

        let server_task = tokio::spawn(async move { server.serve_n(1).await });
        sleep(Duration::from_millis(100)).await;

        let identity_token = create_test_jwt(
            &private_pem,
            "video-user-1",
            &auth_config.apple_bundle_id,
            false,
        );
        let mut client_config = test_client_config(port);
        client_config.request_video_stream = true;

        let mut client = ConnectedClient::connect(&client_config).await.unwrap();
        client.exchange_hello(true).await.unwrap();

        let auth_result = client.authenticate(&identity_token).await.unwrap();
        match auth_result {
            ControlMessage::AuthResult { success, .. } => assert!(success),
            other => panic!("unexpected message: {:?}", other),
        }

        let mut reassembler = H264DatagramReassembler::new(ReassemblerConfig::default());
        let access_unit = client.read_media_access_unit(&mut reassembler).await.unwrap();
        assert_eq!(access_unit.access_unit_id, 1);
        assert_eq!(access_unit.data, test_video_payload());
        assert!(access_unit.is_keyframe);
        assert_eq!(access_unit.pts_100ns, 166_666);
        assert_eq!(access_unit.duration_100ns, 166_666);

        client.close_gracefully().await.unwrap();
        server_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn loopback_resume_restarts_media_stream_on_new_quic_connection() {
        let (private_pem, public_pem) = generate_test_rsa_keypair();
        let tmp = TempDir::new().unwrap();
        let pub_key_path = tmp.path().join("pub.pem");
        std::fs::write(&pub_key_path, &public_pem).unwrap();

        let port = free_port();
        let auth_config = test_auth_config(&tmp, pub_key_path.to_str().unwrap(), 60);
        let server = TransportServer::with_auth(test_video_server_config(port), &auth_config)
            .await
            .unwrap();

        let server_task = tokio::spawn(async move { server.serve_n(2).await });
        sleep(Duration::from_millis(100)).await;

        let identity_token = create_test_jwt(
            &private_pem,
            "video-user-2",
            &auth_config.apple_bundle_id,
            false,
        );
        let mut client_config = test_client_config(port);
        client_config.request_video_stream = true;

        let mut initial_client = ConnectedClient::connect(&client_config).await.unwrap();
        initial_client.exchange_hello(true).await.unwrap();
        let auth_result = initial_client.authenticate(&identity_token).await.unwrap();
        let (session_id, resume_token) = match auth_result {
            ControlMessage::AuthResult {
                success,
                session_id,
                resume_token,
                ..
            } => {
                assert!(success);
                (session_id.unwrap(), resume_token.unwrap())
            }
            other => panic!("unexpected message: {:?}", other),
        };

        let mut first_reassembler = H264DatagramReassembler::new(ReassemblerConfig::default());
        let initial_access_unit = initial_client
            .read_media_access_unit(&mut first_reassembler)
            .await
            .unwrap();
        assert_eq!(initial_access_unit.access_unit_id, 1);
        assert_eq!(initial_access_unit.data, test_video_payload());

        initial_client.close_abrupt().await;
        sleep(Duration::from_millis(100)).await;

        let mut resumed_client = ConnectedClient::connect(&client_config).await.unwrap();
        resumed_client.exchange_hello(true).await.unwrap();
        let resume_result = resumed_client.resume(&resume_token).await.unwrap();
        match resume_result {
            ControlMessage::ResumeResult {
                success,
                session_id: resumed_session_id,
                resume_token: rotated_resume_token,
                ..
            } => {
                assert!(success);
                assert_eq!(resumed_session_id.as_deref(), Some(session_id.as_str()));
                assert_ne!(rotated_resume_token.as_deref(), Some(resume_token.as_str()));
            }
            other => panic!("unexpected message: {:?}", other),
        }

        let mut resumed_reassembler = H264DatagramReassembler::new(ReassemblerConfig::default());
        let resumed_access_unit = resumed_client
            .read_media_access_unit(&mut resumed_reassembler)
            .await
            .unwrap();
        assert_eq!(resumed_access_unit.access_unit_id, 1);
        assert_eq!(resumed_access_unit.data, test_video_payload());
        assert!(resumed_access_unit.is_keyframe);

        resumed_client.close_gracefully().await.unwrap();
        server_task.await.unwrap().unwrap();
    }
}
