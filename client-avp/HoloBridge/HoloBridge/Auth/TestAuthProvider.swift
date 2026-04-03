import Foundation
import Security
import os

/// Test auth provider that generates locally-signed JWTs for development.
///
/// Usage: Run the Rust `auth_smoke_client` binary first to generate the RSA key pair.
/// It writes the private key to `/tmp/holobridge_test_priv.pem` and public key to
/// `/tmp/holobridge_test_pub.pem`. This provider reads the private key to sign JWTs
/// that the Rust host (in test mode) can validate with the matching public key.
///
/// Alternatively, set `privateKeyPemPath` to a custom location.
@MainActor
public final class TestAuthProvider: AuthProvider, @unchecked Sendable {
    private let logger = Logger(subsystem: "HoloBridge", category: "TestAuth")
    private let bundleId: String
    private let subject: String
    private let privateKeyPemPath: String

    public static let defaultPrivateKeyPath = "/tmp/holobridge_test_priv.pem"
    public static let defaultPublicKeyPath = "/tmp/holobridge_test_pub.pem"

    public init(
        bundleId: String? = nil,
        subject: String = "test-user-001",
        privateKeyPemPath: String? = nil
    ) {
        self.bundleId = bundleId ?? Bundle.main.bundleIdentifier ?? "cloud.hr5.HoloBridge"
        self.subject = subject
        self.privateKeyPemPath = privateKeyPemPath ?? Self.defaultPrivateKeyPath
    }

    public func getIdentityToken() async throws -> String {
        let privateKeyPem = try loadPrivateKeyPem()
        let jwt = try createSignedJWT(privateKeyPem: privateKeyPem)
        logger.info("Generated test JWT for subject: \(self.subject)")
        return jwt
    }

    private func loadPrivateKeyPem() throws -> Data {
        guard FileManager.default.fileExists(atPath: privateKeyPemPath) else {
            throw AuthError.testModeKeyNotFound(
                "\(privateKeyPemPath) — run the Rust key generator first: " +
                "cargo run --bin test_keygen"
            )
        }
        guard let data = FileManager.default.contents(atPath: privateKeyPemPath) else {
            throw AuthError.testModeKeyNotFound("Failed to read \(privateKeyPemPath)")
        }
        return data
    }

    private func createSignedJWT(privateKeyPem: Data) throws -> String {
        // Parse PEM to get DER data
        guard let pemString = String(data: privateKeyPem, encoding: .utf8) else {
            throw AuthError.tokenGenerationFailed("Private key is not valid UTF-8")
        }

        let derData = try extractDERFromPEM(pemString)

        // Import the RSA private key
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeRSA,
            kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
        ]

        var error: Unmanaged<CFError>?
        guard let privateKey = SecKeyCreateWithData(derData as CFData, attributes as CFDictionary, &error) else {
            throw AuthError.tokenGenerationFailed("Failed to import RSA private key: \(error!.takeRetainedValue())")
        }

        let now = Int(Date().timeIntervalSince1970)

        // Header
        let header: [String: Any] = [
            "alg": "RS256",
            "typ": "JWT",
            "kid": "test-key-1"
        ]

        // Claims matching what the Rust validator expects
        let claims: [String: Any] = [
            "iss": "https://test.holobridge.local",
            "sub": subject,
            "aud": bundleId,
            "exp": now + 3600,
            "iat": now - 60,
            "email": "\(subject)@test.local",
            "email_verified": true
        ]

        let headerData = try JSONSerialization.data(withJSONObject: header)
        let claimsData = try JSONSerialization.data(withJSONObject: claims)

        let headerB64 = base64urlEncode(headerData)
        let claimsB64 = base64urlEncode(claimsData)
        let signingInput = "\(headerB64).\(claimsB64)"

        guard let inputData = signingInput.data(using: .utf8) else {
            throw AuthError.tokenGenerationFailed("Failed to encode signing input")
        }

        var signError: Unmanaged<CFError>?
        guard let signature = SecKeyCreateSignature(
            privateKey,
            .rsaSignatureMessagePKCS1v15SHA256,
            inputData as CFData,
            &signError
        ) as Data? else {
            throw AuthError.tokenGenerationFailed("Signing failed: \(signError!.takeRetainedValue())")
        }

        let signatureB64 = base64urlEncode(signature)
        return "\(headerB64).\(claimsB64).\(signatureB64)"
    }

    private func extractDERFromPEM(_ pem: String) throws -> Data {
        // Handle both PKCS#1 (RSA PRIVATE KEY) and PKCS#8 (PRIVATE KEY) formats
        let lines = pem.components(separatedBy: "\n")
        let base64Lines = lines.filter { line in
            !line.hasPrefix("-----") && !line.trimmingCharacters(in: .whitespaces).isEmpty
        }
        let base64String = base64Lines.joined()

        guard let data = Data(base64Encoded: base64String) else {
            throw AuthError.tokenGenerationFailed("Failed to decode PEM base64")
        }
        return data
    }

    private func base64urlEncode(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}
