use std::{error::Error, net::SocketAddr, process::ExitCode, time::Duration};

use holobridge_transport::{tls::build_server_config, TransportServerConfig};
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use tokio::{
    time::{sleep, timeout},
};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const STREAM_PING: &[u8] = b"hb-stream-ping-v1";
const STREAM_ACK: &[u8] = b"hb-stream-ack-v1";
const DATAGRAM_PING: &[u8] = b"hb-datagram-ping-v1";
const DATAGRAM_ACK: &[u8] = b"hb-datagram-ack-v1";
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteropMode {
    Stream,
    Datagram,
    Mixed,
}

impl InteropMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "stream" => Ok(Self::Stream),
            "datagram" => Ok(Self::Datagram),
            "mixed" => Ok(Self::Mixed),
            _ => Err(format!("unsupported mode: {value}")),
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(error = %error, "quic interop server failed");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let mode = parse_mode(std::env::args().skip(1))?;
    let config = TransportServerConfig::from_env();
    let server_config = build_server_config(&config)?;
    let bind_addr: SocketAddr = config.listen_endpoint().parse()?;
    let endpoint = Endpoint::server(server_config, bind_addr)?;

    info!(mode = ?mode, endpoint = %bind_addr, alpn = %config.alpn, "quic interop server listening");

    let incoming = timeout(WAIT_TIMEOUT, endpoint.accept())
        .await
        .map_err(|_| "timed out waiting for interop client connection")?
        .ok_or("endpoint closed before accepting interop connection")?;
    let connection = incoming.await?;
    info!(remote = %connection.remote_address(), max_datagram_size = ?connection.max_datagram_size(), "quic interop server accepted connection");

    match mode {
        InteropMode::Stream => run_stream_mode(connection.clone()).await?,
        InteropMode::Datagram => run_datagram_mode(connection.clone()).await?,
        InteropMode::Mixed => run_mixed_mode(connection.clone()).await?,
    }

    connection.close(quinn::VarInt::from_u32(0), b"interop-complete");
    sleep(Duration::from_millis(500)).await;
    endpoint.wait_idle().await;
    Ok(())
}

fn parse_mode(args: impl Iterator<Item = String>) -> Result<InteropMode, Box<dyn Error>> {
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        if argument == "--mode" {
            let value = args.next().ok_or("--mode requires a value")?;
            return Ok(InteropMode::parse(&value)?);
        }
    }

    Err("usage: quic_interop_server --mode <stream|datagram|mixed>".into())
}

async fn run_stream_mode(connection: Connection) -> Result<(), Box<dyn Error>> {
    let (mut send, mut recv) = accept_control_stream(&connection).await?;
    let payload = read_stream_payload(&mut recv, STREAM_PING.len() + 64).await?;
    if payload != STREAM_PING {
        return Err(format!("unexpected stream payload: {:?}", payload).into());
    }

    info!("quic interop server received stream payload");
    send.write_all(STREAM_ACK).await?;
    send.finish()?;
    sleep(Duration::from_millis(500)).await;
    Ok(())
}

async fn run_datagram_mode(connection: Connection) -> Result<(), Box<dyn Error>> {
    info!(
        max_datagram_size = ?connection.max_datagram_size(),
        "datagram mode: waiting for client datagram"
    );

    let payload = timeout(WAIT_TIMEOUT, connection.read_datagram())
        .await
        .map_err(|_| "timed out waiting for datagram from client")??;
    if payload != DATAGRAM_PING {
        return Err(format!("unexpected datagram payload: {:?}", payload).into());
    }
    info!("received datagram ping, sending datagram ack");
    connection.send_datagram(DATAGRAM_ACK.to_vec().into())?;

    sleep(Duration::from_millis(500)).await;
    Ok(())
}

async fn run_mixed_mode(connection: Connection) -> Result<(), Box<dyn Error>> {
    info!(
        max_datagram_size = ?connection.max_datagram_size(),
        "mixed mode: waiting for stream + datagram from client"
    );

    let stream_task = async {
        let (mut send, mut recv) = accept_control_stream(&connection).await?;
        let payload = read_stream_payload(&mut recv, STREAM_PING.len() + 64).await?;
        if payload != STREAM_PING {
            return Err(format!("unexpected mixed-mode stream payload: {:?}", payload).into());
        }
        info!("mixed mode: received stream ping, sending stream ack");
        send.write_all(STREAM_ACK).await?;
        send.finish()?;
        Ok::<_, Box<dyn Error>>(())
    };

    let datagram_task = async {
        let payload = timeout(WAIT_TIMEOUT, connection.read_datagram())
            .await
            .map_err(|_| "timed out waiting for mixed-mode datagram from client")??;
        if payload != DATAGRAM_PING {
            return Err(
                format!("unexpected mixed-mode datagram payload: {:?}", payload).into(),
            );
        }
        info!("mixed mode: received datagram ping, sending datagram ack");
        connection.send_datagram(DATAGRAM_ACK.to_vec().into())?;
        Ok::<_, Box<dyn Error>>(())
    };

    let (stream_result, datagram_result) = tokio::join!(stream_task, datagram_task);
    stream_result?;
    datagram_result?;

    sleep(Duration::from_millis(500)).await;
    Ok(())
}

async fn accept_control_stream(
    connection: &Connection,
) -> Result<(SendStream, RecvStream), Box<dyn Error>> {
    let (send, recv) = timeout(WAIT_TIMEOUT, connection.accept_bi())
        .await
        .map_err(|_| "timed out waiting for bidirectional stream")??;
    info!("quic interop server accepted control stream");
    Ok((send, recv))
}

async fn read_stream_payload(
    recv: &mut RecvStream,
    limit: usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut buffer = vec![0u8; limit];
    let read = timeout(WAIT_TIMEOUT, recv.read(&mut buffer))
        .await
        .map_err(|_| "timed out waiting for stream payload")??;
    let count = read.ok_or("control stream finished before payload arrived")?;
    buffer.truncate(count);
    Ok(buffer)
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
