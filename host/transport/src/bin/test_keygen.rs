use holobridge_auth::test_keys::generate_test_rsa_keypair;
use std::process::ExitCode;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Generate RSA test key pair for development.
/// Writes private and public keys to /tmp/ for use by both
/// the Rust host (public key) and the Swift client (private key).
fn main() -> ExitCode {
    init_tracing();

    let (private_pem, public_pem) = generate_test_rsa_keypair();

    let priv_path = std::env::var("HOLOBRIDGE_AUTH_TEST_PRIVATE_KEY")
        .unwrap_or_else(|_| "/tmp/holobridge_test_priv.pem".to_owned());
    let pub_path = std::env::var("HOLOBRIDGE_AUTH_TEST_PUBLIC_KEY")
        .unwrap_or_else(|_| "/tmp/holobridge_test_pub.pem".to_owned());

    if let Err(e) = std::fs::write(&priv_path, &private_pem) {
        eprintln!("Failed to write private key: {e}");
        return ExitCode::FAILURE;
    }
    info!(path = %priv_path, "wrote test private key");

    if let Err(e) = std::fs::write(&pub_path, &public_pem) {
        eprintln!("Failed to write public key: {e}");
        return ExitCode::FAILURE;
    }
    info!(path = %pub_path, "wrote test public key");

    info!("Test key pair generated. Use these environment variables:");
    info!("  HOLOBRIDGE_AUTH_TEST_PUBLIC_KEY={pub_path}");
    info!("  Private key at: {priv_path} (for Swift client TestAuthProvider)");

    ExitCode::SUCCESS
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
