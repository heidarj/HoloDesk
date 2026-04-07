import Foundation
import HoloBridgeClientCore
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

public typealias SessionState = SessionClientState

public extension SessionClientState {
    var label: String {
        switch self {
        case .disconnected: return "Disconnected"
        case .connecting: return "Connecting..."
        case .authenticating: return "Authenticating..."
        case .resuming: return "Resuming..."
        case .connected(let name): return "Connected" + (name.map { " as \($0)" } ?? "")
        case .error(let message): return "Error: \(message)"
        }
    }
}

@Observable
@MainActor
public final class SessionManager {
    public var authMode: AuthMode
    public let videoRenderer: VideoRenderer
    public private(set) var state: SessionState = .disconnected

    @ObservationIgnored private let logger = Logger(subsystem: "HoloBridge", category: "Session")
    @ObservationIgnored private let videoPipeline: VideoStreamPipeline
    @ObservationIgnored private var sessionClient: SessionClient! = nil

    public init(authMode: AuthMode? = nil) {
        let renderer = VideoRenderer()
        let videoPipeline = VideoStreamPipeline(renderer: renderer)
        self.authMode = authMode ?? Self.defaultAuthMode
        self.videoRenderer = renderer
        self.videoPipeline = videoPipeline
        self.sessionClient = SessionClient(
            transportConfigurationFactory: { endpoint in
                TransportConfiguration(
                    host: endpoint.host,
                    port: endpoint.port,
                    serverName: "localhost",
                    allowInsecureCertificateValidation: true
                )
            },
            transportClientFactory: { [logger] configuration in
                NetworkFrameworkQuicClient(
                    configuration: configuration,
                    diagnosticHandler: { event in
                        let detail = event.detail ?? "-"
                        logger.debug("transport event \(event.kind.rawValue, privacy: .public) detail=\(detail, privacy: .public)")
                    }
                )
            },
            onStateChange: { [weak self] newState in
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    self.state = newState
                    switch newState {
                    case .connected:
                        self.videoPipeline.prepareForStream()
                    case .disconnected, .connecting, .authenticating, .resuming, .error:
                        self.videoPipeline.reset(statusMessage: "Waiting for stream")
                    }
                }
            },
            onVideoDatagram: { [weak self] datagram in
                Task { @MainActor [weak self] in
                    self?.videoPipeline.consume(datagram: datagram)
                }
            }
        )
    }

    public func connect(host: String, port: UInt16) async {
        let endpoint = SessionEndpoint(host: host, port: port)
        let authProvider = makeAuthProvider()
        let identityTokenSupplier: IdentityTokenSupplier = {
            try await Task { @MainActor in
                try await authProvider.getIdentityToken()
            }.value
        }

        do {
            _ = try await sessionClient.connect(
                to: endpoint,
                identityTokenSupplier: identityTokenSupplier,
                requestVideo: true
            )
            logger.info("Session established")
        } catch {
            logger.error("Connection failed: \(error.localizedDescription, privacy: .public)")
            if case .error = state {
                return
            }
            state = .error(error.localizedDescription)
        }
    }

    public func disconnect() async {
        await sessionClient.disconnect(reason: "user-disconnect")
        logger.info("Disconnected")
    }

    public func simulateNetworkDrop() async {
        guard state.isConnected else {
            return
        }
        logger.warning("Simulating unexpected transport loss")
        await sessionClient.simulateNetworkDrop()
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
