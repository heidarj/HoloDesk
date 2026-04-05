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
    case resuming
    case connected(userDisplayName: String?)
    case error(String)

    public var label: String {
        switch self {
        case .disconnected: return "Disconnected"
        case .connecting: return "Connecting..."
        case .authenticating: return "Authenticating..."
        case .resuming: return "Resuming..."
        case .connected(let name): return "Connected" + (name.map { " as \($0)" } ?? "")
        case .error(let msg): return "Error: \(msg)"
        }
    }

    public var isConnected: Bool {
        if case .connected = self { return true }
        return false
    }
}

public enum SessionManagerError: Error, LocalizedError {
    case invalidSessionHandshake(String)

    public var errorDescription: String? {
        switch self {
        case .invalidSessionHandshake(let detail):
            return "Invalid session handshake: \(detail)"
        }
    }
}

private enum ResumeAttemptOutcome {
    case success
    case definitiveFailure(String)
    case transportFailure(Error)
}

private struct SessionBinding {
    let sessionID: String
    let resumeToken: String
    let resumeTokenTTLSeconds: UInt64
    let userDisplayName: String?
}

private struct PreparedTransport {
    let client: any TransportClient
    let generation: Int
    let videoDatagrams: AsyncThrowingStream<Data, Error>
}

@Observable
@MainActor
public final class SessionManager {
    public var authMode: AuthMode
    public let videoRenderer: VideoRenderer
    public private(set) var state: SessionState = .disconnected

    private let logger = Logger(subsystem: "HoloBridge", category: "Session")
    private let videoPipeline: VideoStreamPipeline

    private var transport: (any TransportClient)?
    private var transportGeneration = 0
    private var monitorTask: Task<Void, Never>?
    private var videoMonitorTask: Task<Void, Never>?

    private var lastHost: String?
    private var lastPort: UInt16?
    private var sessionID: String?
    private var resumeToken: String?
    private var resumeTokenExpiry: Date?
    private var userDisplayName: String?
    private var wasUserInitiatedDisconnect = false

    public init(authMode: AuthMode? = nil) {
        let renderer = VideoRenderer()
        self.authMode = authMode ?? Self.defaultAuthMode
        self.videoRenderer = renderer
        self.videoPipeline = VideoStreamPipeline(renderer: renderer)
    }

    public func connect(host: String, port: UInt16) async {
        lastHost = host
        lastPort = port
        wasUserInitiatedDisconnect = false

        await invalidateCurrentTransport(reason: nil)

        if canAttemptResume(to: host, port: port) {
            state = .resuming
            switch await attemptResume(host: host, port: port) {
            case .success:
                return
            case .definitiveFailure(let message):
                logger.warning("Resume rejected during connect: \(message, privacy: .public)")
                clearSessionContext(preserveEndpoint: true)
            case .transportFailure(let error):
                logger.error("Resume transport failed during connect: \(error.localizedDescription, privacy: .public)")
                state = .error(error.localizedDescription)
                return
            }
        }

        await performAuthenticatedConnect(host: host, port: port)
    }

    public func disconnect() async {
        wasUserInitiatedDisconnect = true
        await invalidateCurrentTransport(reason: "user-disconnect")
        clearSessionContext()
        state = .disconnected
        logger.info("Disconnected")
    }

    public func simulateNetworkDrop() async {
        guard state.isConnected, let transport else {
            return
        }

        wasUserInitiatedDisconnect = false
        logger.warning("Simulating unexpected transport loss")
        await transport.close(reason: nil)
    }

    private func performAuthenticatedConnect(host: String, port: UInt16) async {
        state = .connecting

        do {
            let authProvider = makeAuthProvider()
            let preparedTransport = try await openTransport(host: host, port: port)

            logger.info("Sending Hello with media datagram capability")
            try await preparedTransport.client.sendHello(
                clientName: "holobridge-avp",
                capabilities: [
                    ControlMessage.controlStreamCapability,
                    ControlMessage.videoDatagramCapability,
                ]
            )
            let ack = try await preparedTransport.client.awaitHelloAck()
            logger.info("Received HelloAck: \(ack.message ?? "ok", privacy: .public)")

            state = .authenticating
            logger.info("Getting identity token using \(self.authMode.rawValue, privacy: .public) auth mode")
            let token = try await authProvider.getIdentityToken()

            logger.info("Sending Authenticate")
            try await preparedTransport.client.sendAuthenticate(identityToken: token)

            let authResult = try await preparedTransport.client.awaitAuthResult()
            guard authResult.success == true else {
                let reason = authResult.message ?? "unknown"
                logger.warning("Auth rejected: \(reason, privacy: .public)")
                await invalidateTransport(generation: preparedTransport.generation, reason: nil)
                state = .error("Auth rejected: \(reason)")
                return
            }

            let binding = try sessionBinding(from: authResult, action: "auth")
            applyConnectedSession(
                binding,
                transport: preparedTransport.client,
                videoDatagrams: preparedTransport.videoDatagrams,
                generation: preparedTransport.generation
            )
            logger.info("Auth succeeded")
        } catch {
            logger.error("Connection failed: \(error.localizedDescription, privacy: .public)")
            await invalidateCurrentTransport(reason: nil)
            state = .error(error.localizedDescription)
        }
    }

    private func attemptResume(host: String, port: UInt16) async -> ResumeAttemptOutcome {
        guard let resumeToken else {
            return .definitiveFailure("No resume token is available")
        }

        do {
            let preparedTransport = try await openTransport(host: host, port: port)

            logger.info("Sending Hello for resume")
            try await preparedTransport.client.sendHello(
                clientName: "holobridge-avp",
                capabilities: [
                    ControlMessage.controlStreamCapability,
                    ControlMessage.videoDatagramCapability,
                ]
            )
            _ = try await preparedTransport.client.awaitHelloAck()

            logger.info("Sending ResumeSession")
            try await preparedTransport.client.sendResumeSession(resumeToken: resumeToken)
            let resumeResult = try await preparedTransport.client.awaitResumeResult()

            guard resumeResult.success == true else {
                let reason = resumeResult.message ?? "unknown"
                await invalidateTransport(generation: preparedTransport.generation, reason: nil)
                return .definitiveFailure("Resume rejected: \(reason)")
            }

            let binding = try sessionBinding(from: resumeResult, action: "resume")
            applyConnectedSession(
                binding,
                transport: preparedTransport.client,
                videoDatagrams: preparedTransport.videoDatagrams,
                generation: preparedTransport.generation
            )
            logger.info("Session resume succeeded")
            return .success
        } catch {
            await invalidateCurrentTransport(reason: nil)
            return .transportFailure(error)
        }
    }

    private func applyConnectedSession(
        _ binding: SessionBinding,
        transport: any TransportClient,
        videoDatagrams: AsyncThrowingStream<Data, Error>,
        generation: Int
    ) {
        sessionID = binding.sessionID
        resumeToken = binding.resumeToken
        resumeTokenExpiry = Date().addingTimeInterval(TimeInterval(binding.resumeTokenTTLSeconds))
        userDisplayName = binding.userDisplayName
        wasUserInitiatedDisconnect = false
        state = .connected(userDisplayName: binding.userDisplayName)
        startConnectionMonitor(for: transport, generation: generation)
        startVideoMonitor(videoDatagrams, generation: generation)
    }

    private func startConnectionMonitor(
        for transport: any TransportClient,
        generation: Int
    ) {
        monitorTask?.cancel()
        monitorTask = Task { [weak self] in
            guard let self else { return }

            do {
                while !Task.isCancelled {
                    let message = try await transport.receive()
                    await self.handleSessionMessage(message, generation: generation)
                }
            } catch {
                await self.handleTransportTermination(error, generation: generation)
            }
        }
    }

    private func startVideoMonitor(
        _ datagrams: AsyncThrowingStream<Data, Error>,
        generation: Int
    ) {
        videoMonitorTask?.cancel()
        videoMonitorTask = Task { [weak self] in
            guard let self else { return }

            await self.videoPipeline.prepareForStream()

            do {
                for try await datagram in datagrams {
                    if Task.isCancelled {
                        return
                    }
                    await self.videoPipeline.consume(datagram: datagram)
                }

                if !Task.isCancelled {
                    await self.handleVideoTransportTermination(
                        TransportClientError.connectionClosed,
                        generation: generation
                    )
                }
            } catch is CancellationError {
                return
            } catch {
                await self.handleVideoTransportTermination(error, generation: generation)
            }
        }
    }

    private func handleSessionMessage(
        _ message: ControlMessage,
        generation: Int
    ) async {
        guard generation == transportGeneration else {
            return
        }

        switch message.type {
        case .goodbye:
            logger.info("Received remote goodbye")
            wasUserInitiatedDisconnect = true
            await invalidateCurrentTransport(reason: nil)
            clearSessionContext()
            state = .disconnected
        default:
            logger.warning("Ignoring unexpected post-session control message: \(message.kind, privacy: .public)")
        }
    }

    private func handleTransportTermination(
        _ error: Error,
        generation: Int
    ) async {
        guard generation == transportGeneration else {
            return
        }

        transport = nil
        monitorTask = nil
        videoMonitorTask?.cancel()
        videoMonitorTask = nil
        await videoPipeline.reset(statusMessage: "Waiting for stream")

        if wasUserInitiatedDisconnect {
            return
        }

        guard
            let host = lastHost,
            let port = lastPort,
            canAttemptResume(to: host, port: port)
        else {
            state = .error(error.localizedDescription)
            return
        }

        logger.warning("Transport ended unexpectedly, attempting one automatic resume")
        state = .resuming

        switch await attemptResume(host: host, port: port) {
        case .success:
            return
        case .definitiveFailure(let message):
            clearSessionContext(preserveEndpoint: true)
            state = .error(message)
        case .transportFailure(let resumeError):
            state = .error(resumeError.localizedDescription)
        }
    }

    private func handleVideoTransportTermination(
        _ error: Error,
        generation: Int
    ) async {
        guard generation == transportGeneration else {
            return
        }

        logger.warning("Video datagram receive ended: \(error.localizedDescription, privacy: .public)")
        videoRenderer.recordRecoverableIssue(error.localizedDescription)
    }

    private func sessionBinding(
        from message: ControlMessage,
        action: String
    ) throws -> SessionBinding {
        guard let sessionID = message.sessionID, !sessionID.isEmpty else {
            throw SessionManagerError.invalidSessionHandshake("\(action) result missing session_id")
        }
        guard let resumeToken = message.resumeToken, !resumeToken.isEmpty else {
            throw SessionManagerError.invalidSessionHandshake("\(action) result missing resume_token")
        }
        guard let ttl = message.resumeTokenTTLSeconds, ttl > 0 else {
            throw SessionManagerError.invalidSessionHandshake("\(action) result missing resume_token_ttl_secs")
        }

        return SessionBinding(
            sessionID: sessionID,
            resumeToken: resumeToken,
            resumeTokenTTLSeconds: ttl,
            userDisplayName: message.userDisplayName
        )
    }

    private func canAttemptResume(to host: String, port: UInt16) -> Bool {
        guard
            let sessionID,
            !sessionID.isEmpty,
            let resumeToken,
            !resumeToken.isEmpty,
            let expiry = resumeTokenExpiry,
            expiry > Date(),
            lastHost == host,
            lastPort == port
        else {
            return false
        }

        return true
    }

    private func openTransport(
        host: String,
        port: UInt16
    ) async throws -> PreparedTransport {
        let config = TransportConfiguration(
            host: host,
            port: port,
            serverName: "localhost",
            allowInsecureCertificateValidation: true
        )
        let client = NetworkFrameworkQuicClient(configuration: config)
        let generation = beginUsingTransport(client)

        do {
            logger.info("Connecting to \(host, privacy: .public):\(port)")
            try await client.connect()
            let videoDatagrams = client.armVideoDatagramReceive()
            return PreparedTransport(
                client: client,
                generation: generation,
                videoDatagrams: videoDatagrams
            )
        } catch {
            await invalidateTransport(generation: generation, reason: nil)
            throw error
        }
    }

    private func beginUsingTransport(_ transport: any TransportClient) -> Int {
        transportGeneration += 1
        self.transport = transport
        return transportGeneration
    }

    private func invalidateCurrentTransport(reason: String?) async {
        monitorTask?.cancel()
        monitorTask = nil
        videoMonitorTask?.cancel()
        videoMonitorTask = nil
        await videoPipeline.reset(statusMessage: "Waiting for stream")

        guard let transport else {
            self.transport = nil
            return
        }

        self.transport = nil
        transportGeneration += 1
        await transport.close(reason: reason)
    }

    private func invalidateTransport(
        generation: Int,
        reason: String?
    ) async {
        guard generation == transportGeneration else {
            return
        }
        await invalidateCurrentTransport(reason: reason)
    }

    private func clearSessionContext(preserveEndpoint: Bool = false) {
        sessionID = nil
        resumeToken = nil
        resumeTokenExpiry = nil
        userDisplayName = nil
        wasUserInitiatedDisconnect = false
        if !preserveEndpoint {
            lastHost = nil
            lastPort = nil
        }
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
