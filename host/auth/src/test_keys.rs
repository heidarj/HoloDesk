use jsonwebtoken::{encode, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::EncodePublicKey;
use rsa::RsaPrivateKey;
use serde::Serialize;

/// Generate an RSA key pair for test mode and return (private PEM bytes, public PEM bytes).
pub fn generate_test_rsa_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut rng = rand::thread_rng();
    let private_key =
        RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation failed");

    let private_pem = private_key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .expect("PKCS1 PEM encoding failed");

    let public_key = private_key.to_public_key();
    let public_pem = public_key
        .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
        .expect("public key PEM encoding failed");

    (private_pem.as_bytes().to_vec(), public_pem.into_bytes())
}

/// Create a signed JWT for testing.
pub fn create_test_jwt(
    private_key_pem: &[u8],
    sub: &str,
    aud: &str,
    expired: bool,
) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let claims = TestClaims {
        iss: "https://test.holobridge.local".to_owned(),
        sub: sub.to_owned(),
        aud: aud.to_owned(),
        exp: if expired { now - 3600 } else { now + 3600 },
        iat: now - 60,
        email: Some(format!("{sub}@test.local")),
        email_verified: Some(true),
    };

    let key =
        EncodingKey::from_rsa_pem(private_key_pem).expect("encoding key from test PEM");

    let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("test-key-1".to_owned());

    encode(&header, &claims, &key).expect("JWT encoding failed")
}

#[derive(Serialize)]
struct TestClaims {
    iss: String,
    sub: String,
    aud: String,
    exp: u64,
    iat: u64,
    email: Option<String>,
    email_verified: Option<bool>,
}
