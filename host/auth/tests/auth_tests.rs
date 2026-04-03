use holobridge_auth::{
    config::AuthConfig,
    test_keys::{create_test_jwt, generate_test_rsa_keypair},
    validator::TokenValidator,
    user_store::AuthorizedUserStore,
};
use std::path::PathBuf;
use tempfile::TempDir;

fn test_auth_config(tmp: &TempDir, pub_key_path: &str) -> AuthConfig {
    AuthConfig {
        apple_bundle_id: "com.holobridge.client".to_owned(),
        jwks_cache_ttl_secs: 3600,
        user_store_path: tmp.path().join("users.json"),
        bootstrap_mode: true,
        test_mode: true,
        test_public_key_pem: Some(PathBuf::from(pub_key_path)),
    }
}

#[tokio::test]
async fn test_valid_token_validation() {
    let (private_pem, public_pem) = generate_test_rsa_keypair();
    let tmp = TempDir::new().unwrap();
    let pub_key_path = tmp.path().join("pub.pem");
    std::fs::write(&pub_key_path, &public_pem).unwrap();

    let config = test_auth_config(&tmp, pub_key_path.to_str().unwrap());
    let validator = TokenValidator::new(&config).await.unwrap();

    let token = create_test_jwt(&private_pem, "user-123", "com.holobridge.client", false);
    let claims = validator.validate(&token).await.unwrap();

    assert_eq!(claims.sub, "user-123");
    assert_eq!(claims.aud, "com.holobridge.client");
    assert_eq!(claims.iss, "https://test.holobridge.local");
}

#[tokio::test]
async fn test_expired_token_rejected() {
    let (private_pem, public_pem) = generate_test_rsa_keypair();
    let tmp = TempDir::new().unwrap();
    let pub_key_path = tmp.path().join("pub.pem");
    std::fs::write(&pub_key_path, &public_pem).unwrap();

    let config = test_auth_config(&tmp, pub_key_path.to_str().unwrap());
    let validator = TokenValidator::new(&config).await.unwrap();

    let token = create_test_jwt(&private_pem, "user-123", "com.holobridge.client", true);
    let result = validator.validate(&token).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("expired"),
        "expected expired error, got: {err}"
    );
}

#[tokio::test]
async fn test_wrong_audience_rejected() {
    let (private_pem, public_pem) = generate_test_rsa_keypair();
    let tmp = TempDir::new().unwrap();
    let pub_key_path = tmp.path().join("pub.pem");
    std::fs::write(&pub_key_path, &public_pem).unwrap();

    let config = test_auth_config(&tmp, pub_key_path.to_str().unwrap());
    let validator = TokenValidator::new(&config).await.unwrap();

    let token = create_test_jwt(&private_pem, "user-123", "com.wrong.bundle", false);
    let result = validator.validate(&token).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("audience"),
        "expected audience error, got: {err}"
    );
}

#[tokio::test]
async fn test_user_store_bootstrap() {
    let tmp = TempDir::new().unwrap();
    let store_path = tmp.path().join("users.json");
    let store = AuthorizedUserStore::load(&store_path, true).await.unwrap();

    // Initially empty, no users authorized.
    assert!(!store.is_authorized("user-1").await);
    assert_eq!(store.user_count().await, 0);

    // Bootstrap: first user is auto-registered.
    let authorized = store.check_or_bootstrap("user-1", Some("Test User")).await.unwrap();
    assert!(authorized);
    assert_eq!(store.user_count().await, 1);
    assert!(store.is_authorized("user-1").await);

    // Second user is NOT auto-registered (bootstrap only registers first user).
    let authorized2 = store.check_or_bootstrap("user-2", None).await.unwrap();
    assert!(!authorized2);
    assert_eq!(store.user_count().await, 1);
}

#[tokio::test]
async fn test_user_store_no_bootstrap() {
    let tmp = TempDir::new().unwrap();
    let store_path = tmp.path().join("users.json");
    let store = AuthorizedUserStore::load(&store_path, false).await.unwrap();

    // Without bootstrap, even the first user is rejected.
    let authorized = store.check_or_bootstrap("user-1", None).await.unwrap();
    assert!(!authorized);
}

#[tokio::test]
async fn test_user_store_persistence() {
    let tmp = TempDir::new().unwrap();
    let store_path = tmp.path().join("users.json");

    // Register a user and drop the store.
    {
        let store = AuthorizedUserStore::load(&store_path, true).await.unwrap();
        store.register_user("user-1", Some("Test User")).await.unwrap();
    }

    // Reload and verify the user persists.
    let store = AuthorizedUserStore::load(&store_path, false).await.unwrap();
    assert!(store.is_authorized("user-1").await);
    assert_eq!(store.user_count().await, 1);
}
