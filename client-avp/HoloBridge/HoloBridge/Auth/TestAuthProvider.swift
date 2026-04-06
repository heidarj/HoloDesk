import Foundation
import HoloBridgeClientTestAuth
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
    private let supplier: TestIdentityTokenSupplier

    public static let defaultPrivateKeyPath = TestIdentityTokenSupplier.defaultPrivateKeyPath
    public static let defaultPublicKeyPath = TestIdentityTokenSupplier.defaultPublicKeyPath

    public init(
        bundleId: String? = nil,
        subject: String = "test-user-001",
        privateKeyPemPath: String? = nil
    ) {
        self.supplier = TestIdentityTokenSupplier(
            bundleID: bundleId ?? Bundle.main.bundleIdentifier ?? TestIdentityTokenSupplier.defaultBundleID,
            subject: subject,
            privateKeyPEMPath: privateKeyPemPath ?? Self.defaultPrivateKeyPath
        )
    }

    public func getIdentityToken() async throws -> String {
        do {
            let jwt = try supplier.getIdentityToken()
            logger.info("Generated test JWT for subject: \(self.supplier.subject)")
            return jwt
        } catch let error as TestIdentityTokenError {
            switch error {
            case .keyNotFound(let path):
                throw AuthError.testModeKeyNotFound(path)
            case .invalidPrivateKeyEncoding, .tokenGenerationFailed:
                throw AuthError.tokenGenerationFailed(error.localizedDescription)
            }
        }
    }
}
