use std::process::ExitCode;

use holobridge_transport::{TransportClientConfig, TransportSmokeClient};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    init_tracing();

    let client = TransportSmokeClient::new(TransportClientConfig::from_env());
    let summary = client.runtime_summary();

    info!(endpoint = %summary.remote_endpoint, alpn = %summary.alpn, validation = %summary.validation, close_mode = summary.close_mode, "prepared live smoke client configuration");

    match client.run() {
        Ok(()) => {
            ExitCode::SUCCESS
        }
        Err(error) => {
            error!(error = %error, "live MsQuic smoke client remains incomplete; transcript-only success paths were removed from the default binary");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}