use std::{
    env,
    fs::File,
    io::Write,
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

use holobridge_auth::test_keys::create_test_jwt;
use holobridge_transport::{
    tls::build_client_config, ControlMessage, ControlMessageCodec, FrameAccumulator,
    H264DatagramReassembler, ReassemblerConfig, TransportClientConfig, CONTROL_STREAM_CAPABILITY,
    VIDEO_DATAGRAM_CAPABILITY,
};
use quinn::{Endpoint, RecvStream, SendStream};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(error = %error, "video smoke client failed");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let options = SmokeOptions::parse(env::args().skip(1))?;

    let priv_key_path = env::var("HOLOBRIDGE_AUTH_TEST_PRIVATE_KEY")
        .unwrap_or_else(|_| "/tmp/holobridge_test_priv.pem".to_owned());
    let private_pem = std::fs::read(&priv_key_path)
        .map_err(|error| format!("failed to read test private key {priv_key_path}: {error}"))?;
    let bundle_id =
        env::var("HOLOBRIDGE_AUTH_BUNDLE_ID").unwrap_or_else(|_| "cloud.hr5.HoloBridge".to_owned());

    let identity_token = create_test_jwt(&private_pem, &options.test_user_sub, &bundle_id, false);
    let mut config = TransportClientConfig::from_env();
    config.identity_token = Some(identity_token);
    config.request_video_stream = true;

    let client_config = build_client_config(&config).map_err(|error| error.to_string())?;
    let mut endpoint =
        Endpoint::client("0.0.0.0:0".parse().unwrap()).map_err(|error| error.to_string())?;
    endpoint.set_default_client_config(client_config);

    let server_addr = config
        .remote_endpoint()
        .parse()
        .map_err(|_| format!("invalid endpoint: {}", config.remote_endpoint()))?;
    let server_name = config
        .server_name
        .clone()
        .unwrap_or_else(|| config.server_host.clone());
    let connection = endpoint
        .connect(server_addr, &server_name)
        .map_err(|error| error.to_string())?
        .await
        .map_err(|error| error.to_string())?;

    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|error| error.to_string())?;
    let mut accumulator = FrameAccumulator::default();

    send_message(
        &mut send,
        &ControlMessage::hello(
            "video-smoke-client",
            vec![
                CONTROL_STREAM_CAPABILITY.to_owned(),
                VIDEO_DATAGRAM_CAPABILITY.to_owned(),
            ],
        ),
    )
    .await?;
    let hello_messages = read_messages(&mut recv, &mut accumulator).await?;
    if !hello_messages
        .iter()
        .any(|message| matches!(message, ControlMessage::HelloAck { .. }))
    {
        return Err("server did not return hello_ack".to_owned());
    }

    let identity_token = config
        .identity_token
        .as_deref()
        .ok_or_else(|| "identity token missing from client config".to_owned())?;
    send_message(&mut send, &ControlMessage::authenticate(identity_token)).await?;

    let auth_messages = read_messages(&mut recv, &mut accumulator).await?;
    let auth_result = auth_messages
        .into_iter()
        .find(|message| matches!(message, ControlMessage::AuthResult { .. }))
        .ok_or_else(|| "server did not return auth_result".to_owned())?;
    match auth_result {
        ControlMessage::AuthResult {
            success, message, ..
        } if success => {
            info!(message, "authenticated for video smoke");
        }
        ControlMessage::AuthResult { message, .. } => {
            return Err(format!("auth failed: {message}"));
        }
        other => {
            return Err(format!("unexpected control message: {:?}", other));
        }
    }

    let mut output = File::create(&options.output)
        .map_err(|error| format!("failed to create {}: {error}", options.output.display()))?;
    let mut reassembler = H264DatagramReassembler::new(ReassemblerConfig::default());
    let deadline = Instant::now() + Duration::from_secs(options.duration_seconds);
    let mut completed_access_units = 0u64;
    let mut keyframes = 0u64;
    let mut total_bytes = 0u64;

    while Instant::now() < deadline {
        let datagram = match tokio::time::timeout(
            Duration::from_millis(250),
            connection.read_datagram(),
        )
        .await
        {
            Ok(Ok(datagram)) => datagram,
            Ok(Err(error)) => return Err(format!("failed to read datagram: {error}")),
            Err(_) => continue,
        };

        if let Some(access_unit) = reassembler
            .push_datagram(&datagram, Instant::now())
            .map_err(|error| error.to_string())?
        {
            output
                .write_all(&access_unit.data)
                .map_err(|error| format!("failed to write H.264 output: {error}"))?;
            completed_access_units += 1;
            keyframes += u64::from(access_unit.is_keyframe);
            total_bytes += access_unit.data.len() as u64;
        }
    }

    if let Err(error) =
        send_message(&mut send, &ControlMessage::goodbye("video-smoke-complete")).await
    {
        warn!(error = %error, "failed to send goodbye");
    }
    let _ = send.finish();
    connection.close(quinn::VarInt::from_u32(0), b"video-smoke-complete");
    endpoint.wait_idle().await;

    println!("output_file: {}", options.output.display());
    println!("completed_access_units: {completed_access_units}");
    println!("keyframes: {keyframes}");
    println!("total_bytes: {total_bytes}");
    println!(
        "dropped_incomplete_access_units: {}",
        reassembler.stats().dropped_incomplete_access_units
    );

    Ok(())
}

async fn send_message(send: &mut SendStream, message: &ControlMessage) -> Result<(), String> {
    let encoded = ControlMessageCodec::encode(message).map_err(|error| error.to_string())?;
    send.write_all(&encoded)
        .await
        .map_err(|error| error.to_string())
}

async fn read_messages(
    recv: &mut RecvStream,
    accumulator: &mut FrameAccumulator,
) -> Result<Vec<ControlMessage>, String> {
    loop {
        let messages = accumulator
            .drain_messages()
            .map_err(|error| error.to_string())?;
        if !messages.is_empty() {
            return Ok(messages);
        }

        let mut buf = vec![0u8; 4096];
        match recv.read(&mut buf).await {
            Ok(Some(n)) => accumulator.push(&buf[..n]),
            Ok(None) => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        }
    }
}

struct SmokeOptions {
    duration_seconds: u64,
    output: PathBuf,
    test_user_sub: String,
}

impl SmokeOptions {
    fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            duration_seconds: 5,
            output: PathBuf::from("holobridge-video-smoke.h264"),
            test_user_sub: "video-smoke-user".to_owned(),
        };

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--duration-seconds" => {
                    options.duration_seconds = arguments
                        .next()
                        .ok_or_else(|| "--duration-seconds requires a value".to_owned())?
                        .parse::<u64>()
                        .map_err(|error| format!("invalid --duration-seconds value: {error}"))?;
                }
                "--output" => {
                    options.output = PathBuf::from(
                        arguments
                            .next()
                            .ok_or_else(|| "--output requires a value".to_owned())?,
                    );
                }
                "--test-user-sub" => {
                    options.test_user_sub = arguments
                        .next()
                        .ok_or_else(|| "--test-user-sub requires a value".to_owned())?;
                }
                "--help" | "-h" => {
                    return Err(Self::usage().to_owned());
                }
                other => {
                    return Err(format!("unknown argument: {other}\n{}", Self::usage()));
                }
            }
        }

        Ok(options)
    }

    fn usage() -> &'static str {
        "usage: video_smoke_client [--duration-seconds N] [--output path] [--test-user-sub value]"
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
