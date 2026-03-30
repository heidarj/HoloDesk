use std::process::ExitCode;

use holobridge_transport::{TransportServer, TransportServerConfig};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    init_tracing();

    let server = TransportServer::new(TransportServerConfig::from_env());
    match server.runtime_summary() {
        Ok(summary) => {
            info!(backend = summary.backend, endpoint = %summary.bind_endpoint, alpn = %summary.alpn, certificate = %summary.certificate, close_mode = summary.close_mode, "prepared live host transport configuration");

            match server.serve_once() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    error!(error = %error, "live MsQuic host runtime remains incomplete; transcript-only success paths were removed from the default binary");
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            error!(error = %error, "failed to prepare live host transport configuration");
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