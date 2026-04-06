import Foundation
import HoloBridgeClientCore
import Security

public enum TestIdentityTokenError: Error, LocalizedError {
    case keyNotFound(String)
    case invalidPrivateKeyEncoding
    case tokenGenerationFailed(String)

    public var errorDescription: String? {
        switch self {
        case .keyNotFound(let path):
            return "Test mode private key not found at: \(path)"
        case .invalidPrivateKeyEncoding:
            return "Private key PEM is not valid UTF-8"
        case .tokenGenerationFailed(let detail):
            return "Test token generation failed: \(detail)"
        }
    }
}

public struct TestIdentityTokenSupplier: Sendable {
    public static let defaultPrivateKeyPath = "/tmp/holobridge_test_priv.pem"
    public static let defaultPublicKeyPath = "/tmp/holobridge_test_pub.pem"
    public static let defaultBundleID = "cloud.hr5.HoloBridge"

    public let bundleID: String
    public let subject: String
    public let privateKeyPEMPath: String

    public init(
        bundleID: String = TestIdentityTokenSupplier.defaultBundleID,
        subject: String = "test-user-001",
        privateKeyPEMPath: String = TestIdentityTokenSupplier.defaultPrivateKeyPath
    ) {
        self.bundleID = bundleID
        self.subject = subject
        self.privateKeyPEMPath = privateKeyPEMPath
    }

    public func makeSupplier() -> IdentityTokenSupplier {
        { [self] in
            try self.getIdentityToken()
        }
    }

    public func getIdentityToken() throws -> String {
        let privateKeyPEM = try loadPrivateKeyPEM()
        return try createSignedJWT(privateKeyPEM: privateKeyPEM)
    }

    private func loadPrivateKeyPEM() throws -> Data {
        guard FileManager.default.fileExists(atPath: privateKeyPEMPath) else {
            throw TestIdentityTokenError.keyNotFound(
                "\(privateKeyPEMPath) — run the Rust key generator first: cargo build -p holobridge-transport --bin test_keygen && ./host/target/debug/test_keygen"
            )
        }
        guard let data = FileManager.default.contents(atPath: privateKeyPEMPath) else {
            throw TestIdentityTokenError.keyNotFound("Failed to read \(privateKeyPEMPath)")
        }
        return data
    }

    private func createSignedJWT(privateKeyPEM: Data) throws -> String {
        guard let pemString = String(data: privateKeyPEM, encoding: .utf8) else {
            throw TestIdentityTokenError.invalidPrivateKeyEncoding
        }

        let derData = try extractDERFromPEM(pemString)
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeRSA,
            kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
        ]

        var error: Unmanaged<CFError>?
        guard let privateKey = SecKeyCreateWithData(derData as CFData, attributes as CFDictionary, &error) else {
            throw TestIdentityTokenError.tokenGenerationFailed(
                "Failed to import RSA private key: \(String(describing: error?.takeRetainedValue()))"
            )
        }

        let now = Int(Date().timeIntervalSince1970)
        let header: [String: Any] = [
            "alg": "RS256",
            "typ": "JWT",
            "kid": "test-key-1",
        ]
        let claims: [String: Any] = [
            "iss": "https://test.holobridge.local",
            "sub": subject,
            "aud": bundleID,
            "exp": now + 3600,
            "iat": now - 60,
            "email": "\(subject)@test.local",
            "email_verified": true,
        ]

        let headerData = try JSONSerialization.data(withJSONObject: header)
        let claimsData = try JSONSerialization.data(withJSONObject: claims)

        let headerB64 = base64urlEncode(headerData)
        let claimsB64 = base64urlEncode(claimsData)
        let signingInput = "\(headerB64).\(claimsB64)"

        guard let inputData = signingInput.data(using: .utf8) else {
            throw TestIdentityTokenError.tokenGenerationFailed("Failed to encode signing input")
        }

        var signError: Unmanaged<CFError>?
        guard let signature = SecKeyCreateSignature(
            privateKey,
            .rsaSignatureMessagePKCS1v15SHA256,
            inputData as CFData,
            &signError
        ) as Data? else {
            throw TestIdentityTokenError.tokenGenerationFailed(
                "Signing failed: \(String(describing: signError?.takeRetainedValue()))"
            )
        }

        let signatureB64 = base64urlEncode(signature)
        return "\(headerB64).\(claimsB64).\(signatureB64)"
    }

    private func extractDERFromPEM(_ pem: String) throws -> Data {
        let lines = pem.components(separatedBy: "\n")
        let base64Lines = lines.filter { line in
            !line.hasPrefix("-----") && !line.trimmingCharacters(in: .whitespaces).isEmpty
        }
        let base64String = base64Lines.joined()

        guard let data = Data(base64Encoded: base64String) else {
            throw TestIdentityTokenError.tokenGenerationFailed("Failed to decode PEM base64")
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
