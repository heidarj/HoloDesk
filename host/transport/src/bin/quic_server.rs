use std::process::ExitCode;

use holobridge_auth::AuthConfig;
use holobridge_transport::{TransportServer, TransportServerConfig};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    let transport_config = TransportServerConfig::from_env();
    let auth_config = AuthConfig::from_env();

    let server = if auth_config.test_mode || !auth_config.apple_bundle_id.is_empty() {
        info!("auth enabled (test_mode={})", auth_config.test_mode);
        match TransportServer::with_auth(transport_config, &auth_config).await {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "failed to initialize auth");
                return ExitCode::FAILURE;
            }
        }
    } else {
        info!("auth disabled (no bundle ID configured)");
        TransportServer::new(transport_config)
    };

    let summary = server.runtime_summary();
    info!(backend = summary.backend, endpoint = %summary.bind_endpoint, alpn = %summary.alpn, certificate = %summary.certificate, close_mode = summary.close_mode, "prepared host transport configuration");

    match server.serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!(error = %error, "host transport failed");
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
