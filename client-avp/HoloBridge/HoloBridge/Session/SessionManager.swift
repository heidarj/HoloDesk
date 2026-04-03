import Foundation
import os

public enum AuthMode: String, CaseIterable, Equatable, Hashable, Identifiable, Sendable {
    case apple
    case test

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .apple:
            return "Apple"
        case .test:
            return "Test"
        }
    }
}

public enum SessionState: Equatable, Sendable {
    case disconnected
    case connecting
    case authenticating
    case connected(userDisplayName: String?)
    case error(String)

    public var label: String {
        switch self {
        case .disconnected: return "Disconnected"
        case .connecting: return "Connecting..."
        case .authenticating: return "Authenticating..."
        case .connected(let name): return "Connected" + (name.map { " as \($0)" } ?? "")
        case .error(let msg): return "Error: \(msg)"
        }
    }

    public var isConnected: Bool {
        if case .connected = self { return true }
        return false
    }
}

/// Orchestrates the full connection + auth flow.
@Observable
@MainActor
public final class SessionManager {
    public var authMode: AuthMode
    public private(set) var state: SessionState = .disconnected

    private let logger = Logger(subsystem: "HoloBridge", category: "Session")
    private var transport: (any TransportClient)?

    public init(authMode: AuthMode? = nil) {
        self.authMode = authMode ?? Self.defaultAuthMode
    }

    public func connect(host: String, port: UInt16) async {
        state = .connecting

        do {
            let authProvider = makeAuthProvider()

            // Create transport
            let config = TransportConfiguration(
                host: host,
                port: port,
                serverName: "localhost",
                allowInsecureCertificateValidation: true
            )
            let client = NetworkFrameworkQuicClient(configuration: config)
            self.transport = client

            // Connect QUIC
            logger.info("Connecting to \(host):\(port)")
            try await client.connect()

            // Hello / HelloAck
            logger.info("Sending Hello")
            try await client.sendHello()
            let ack = try await client.awaitHelloAck()
            logger.info("Received HelloAck: \(ack.message ?? "ok")")

            // Auth
            state = .authenticating
            logger.info("Getting identity token using \(self.authMode.rawValue, privacy: .public) auth mode")
            let token = try await authProvider.getIdentityToken()

            logger.info("Sending Authenticate")
            try await client.sendAuthenticate(identityToken: token)

            let authResult = try await client.awaitAuthResult()
            guard authResult.success == true else {
                let reason = authResult.message ?? "unknown"
                logger.warning("Auth rejected: \(reason)")
                state = .error("Auth rejected: \(reason)")
                await client.close(reason: nil)
                self.transport = nil
                return
            }

            logger.info("Auth succeeded")
            state = .connected(userDisplayName: authResult.userDisplayName)

        } catch {
            logger.error("Connection failed: \(error.localizedDescription)")
            state = .error(error.localizedDescription)
            if let t = transport {
                await t.close(reason: nil)
            }
            transport = nil
        }
    }

    public func disconnect() async {
        if let t = transport {
            await t.close(reason: "user-disconnect")
        }
        transport = nil
        state = .disconnected
        logger.info("Disconnected")
    }

    private func makeAuthProvider() -> any AuthProvider {
        switch authMode {
        case .apple:
            return AppleAuthProvider()
        case .test:
            return TestAuthProvider()
        }
    }

    nonisolated private static var defaultAuthMode: AuthMode {
        #if DEBUG
        .test
        #else
        .apple
        #endif
    }
}
