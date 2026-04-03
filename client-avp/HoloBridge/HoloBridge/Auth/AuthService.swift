import AuthenticationServices
import Foundation
import UIKit
import os

public enum AuthError: Error, LocalizedError {
    case signInFailed(String)
    case noIdentityToken
    case invalidIdentityTokenEncoding
    case presentationAnchorUnavailable
    case requestAlreadyInProgress
    case testModeKeyNotFound(String)
    case tokenGenerationFailed(String)

    public var errorDescription: String? {
        switch self {
        case .signInFailed(let detail):
            return "Sign in with Apple failed: \(detail)"
        case .noIdentityToken:
            return "Sign in with Apple succeeded but no identity token was returned"
        case .invalidIdentityTokenEncoding:
            return "Sign in with Apple returned an identity token that could not be decoded as UTF-8"
        case .presentationAnchorUnavailable:
            return "No active window is available to present Sign in with Apple"
        case .requestAlreadyInProgress:
            return "A Sign in with Apple request is already in progress"
        case .testModeKeyNotFound(let path):
            return "Test mode private key not found at: \(path)"
        case .tokenGenerationFailed(let detail):
            return "Test token generation failed: \(detail)"
        }
    }
}

/// Provides identity tokens for authentication.
/// In production: Sign in with Apple.
/// In test mode: generates a locally-signed JWT matching the Rust host's test validator.
@MainActor
public protocol AuthProvider: Sendable {
    func getIdentityToken() async throws -> String
}

/// Sign in with Apple provider for production use.
@MainActor
public final class AppleAuthProvider: NSObject, AuthProvider, ASAuthorizationControllerDelegate, ASAuthorizationControllerPresentationContextProviding, @unchecked Sendable {
    private let logger = Logger(subsystem: "HoloBridge", category: "AppleAuth")
    private var continuation: CheckedContinuation<String, Error>?
    private var presentationAnchor: ASPresentationAnchor?

    public override init() {
        super.init()
    }

    public func getIdentityToken() async throws -> String {
        guard continuation == nil else {
            throw AuthError.requestAlreadyInProgress
        }

        guard let presentationAnchor = Self.findPresentationAnchor() else {
            throw AuthError.presentationAnchorUnavailable
        }

        return try await withCheckedThrowingContinuation { continuation in
            self.continuation = continuation
            self.presentationAnchor = presentationAnchor

            let provider = ASAuthorizationAppleIDProvider()
            let request = provider.createRequest()
            request.requestedScopes = [.email]

            let controller = ASAuthorizationController(authorizationRequests: [request])
            controller.delegate = self
            controller.presentationContextProvider = self
            controller.performRequests()
        }
    }

    public func authorizationController(
        controller: ASAuthorizationController,
        didCompleteWithAuthorization authorization: ASAuthorization
    ) {
        guard let credential = authorization.credential as? ASAuthorizationAppleIDCredential,
              let tokenData = credential.identityToken else {
            finish(with: .failure(AuthError.noIdentityToken))
            return
        }

        guard let token = String(data: tokenData, encoding: .utf8) else {
            finish(with: .failure(AuthError.invalidIdentityTokenEncoding))
            return
        }

        logger.info("Received Apple identity token")
        finish(with: .success(token))
    }

    public func authorizationController(
        controller: ASAuthorizationController,
        didCompleteWithError error: Error
    ) {
        finish(with: .failure(AuthError.signInFailed(error.localizedDescription)))
    }

    public func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
        guard let presentationAnchor else {
            fatalError("Sign in with Apple presentation anchor was not configured")
        }
        return presentationAnchor
    }

    private func finish(with result: Result<String, Error>) {
        continuation?.resume(with: result)
        continuation = nil
        presentationAnchor = nil
    }

    private static func findPresentationAnchor() -> ASPresentationAnchor? {
        for case let scene as UIWindowScene in UIApplication.shared.connectedScenes {
            if let keyWindow = scene.windows.first(where: \.isKeyWindow) {
                return keyWindow
            }
        }

        for case let scene as UIWindowScene in UIApplication.shared.connectedScenes {
            if let window = scene.windows.first {
                return window
            }
        }

        return nil
    }
}
