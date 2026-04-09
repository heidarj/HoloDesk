import Foundation

public struct SessionEndpoint: Sendable, Equatable {
    public let host: String
    public let port: UInt16

    public init(host: String, port: UInt16) {
        self.host = host
        self.port = port
    }
}

public struct SessionBinding: Sendable, Equatable {
    public let sessionID: String
    public let resumeToken: String
    public let resumeTokenTTLSeconds: UInt64
    public let userDisplayName: String?

    public init(
        sessionID: String,
        resumeToken: String,
        resumeTokenTTLSeconds: UInt64,
        userDisplayName: String?
    ) {
        self.sessionID = sessionID
        self.resumeToken = resumeToken
        self.resumeTokenTTLSeconds = resumeTokenTTLSeconds
        self.userDisplayName = userDisplayName
    }
}

public enum SessionClientState: Equatable, Sendable {
    case disconnected
    case connecting
    case authenticating
    case resuming
    case connected(userDisplayName: String?)
    case error(String)
}

public enum SessionClientError: Error, LocalizedError {
    case invalidSessionHandshake(String)
    case authRejected(String)
    case notConnected

    public var errorDescription: String? {
        switch self {
        case .invalidSessionHandshake(let detail):
            return "Invalid session handshake: \(detail)"
        case .authRejected(let detail):
            return "Authentication failed: \(detail)"
        case .notConnected:
            return "The session is not connected"
        }
    }
}

public typealias IdentityTokenSupplier = @Sendable () async throws -> String
public typealias VideoDatagramHandler = @Sendable (Data) -> Void
public typealias PointerShapeMessageHandler = @Sendable (ControlMessage) -> Void
public typealias StateChangeHandler = @Sendable (SessionClientState) -> Void
public typealias TransportClientFactory = @Sendable (TransportConfiguration) -> any TransportClient
public typealias TransportConfigurationFactory = @Sendable (SessionEndpoint) -> TransportConfiguration

private enum ResumeAttemptOutcome {
    case success(SessionBinding)
    case definitiveFailure(String)
    case transportFailure(Error)
}

private struct PreparedTransport {
    let client: any TransportClient
    let generation: Int
    let videoDatagrams: AsyncThrowingStream<Data, Error>
}

public actor SessionClient {
    public private(set) var state: SessionClientState = .disconnected
    public private(set) var binding: SessionBinding?

    private let transportConfigurationFactory: TransportConfigurationFactory
    private let transportClientFactory: TransportClientFactory
    private let onStateChange: StateChangeHandler?
    private let onVideoDatagram: VideoDatagramHandler?
    private let onPointerShapeMessage: PointerShapeMessageHandler?

    private var transport: (any TransportClient)?
    private var transportGeneration = 0
    private var monitorTask: Task<Void, Never>?
    private var videoMonitorTask: Task<Void, Never>?

    private var lastEndpoint: SessionEndpoint?
    private var resumeTokenExpiry: Date?
    private var wasUserInitiatedDisconnect = false
    private var requestVideo = true

    public init(
        transportConfigurationFactory: @escaping TransportConfigurationFactory = { endpoint in
            TransportConfiguration(
                host: endpoint.host,
                port: endpoint.port,
                serverName: "localhost",
                allowInsecureCertificateValidation: true
            )
        },
        transportClientFactory: @escaping TransportClientFactory = { configuration in
            if #available(visionOS 1.0, iOS 15.0, macOS 12.0, *) {
                return NetworkFrameworkQuicClient(configuration: configuration)
            }

            fatalError("NetworkFrameworkQuicClient requires macOS 12.0 or newer")
        },
        onStateChange: StateChangeHandler? = nil,
        onVideoDatagram: VideoDatagramHandler? = nil,
        onPointerShapeMessage: PointerShapeMessageHandler? = nil
    ) {
        self.transportConfigurationFactory = transportConfigurationFactory
        self.transportClientFactory = transportClientFactory
        self.onStateChange = onStateChange
        self.onVideoDatagram = onVideoDatagram
        self.onPointerShapeMessage = onPointerShapeMessage
    }

    @discardableResult
    public func connect(
        to endpoint: SessionEndpoint,
        identityTokenSupplier: @escaping IdentityTokenSupplier,
        requestVideo: Bool = true
    ) async throws -> SessionBinding {
        lastEndpoint = endpoint
        wasUserInitiatedDisconnect = false
        self.requestVideo = requestVideo

        await invalidateCurrentTransport(reason: nil)

        if canAttemptResume(to: endpoint) {
            setState(.resuming)
            switch await attemptResume(endpoint: endpoint, requestVideo: requestVideo) {
            case .success(let resumedBinding):
                return resumedBinding
            case .definitiveFailure:
                clearSessionContext(preserveEndpoint: true)
            case .transportFailure(let error):
                setState(.error(error.localizedDescription))
                throw error
            }
        }

        setState(.connecting)

        do {
            let preparedTransport = try await openTransport(endpoint: endpoint)

            try await preparedTransport.client.sendHello(
                clientName: "holobridge-avp",
                capabilities: capabilities(for: requestVideo)
            )
            _ = try await preparedTransport.client.awaitHelloAck()

            setState(.authenticating)
            let token = try await identityTokenSupplier()
            try await preparedTransport.client.sendAuthenticate(identityToken: token)

            let authResult = try await preparedTransport.client.awaitAuthResult()
            guard authResult.success == true else {
                let reason = authResult.message ?? "unknown"
                await invalidateTransport(generation: preparedTransport.generation, reason: nil)
                let error = SessionClientError.authRejected(reason)
                setState(.error(error.localizedDescription))
                throw error
            }

            let binding = try sessionBinding(from: authResult, action: "auth")
            applyConnectedSession(
                binding,
                transport: preparedTransport.client,
                videoDatagrams: preparedTransport.videoDatagrams,
                generation: preparedTransport.generation
            )
            return binding
        } catch {
            await invalidateCurrentTransport(reason: nil)
            if case .error = state {
                throw error
            }
            setState(.error(error.localizedDescription))
            throw error
        }
    }

    public func disconnect(reason: String? = "user-disconnect") async {
        wasUserInitiatedDisconnect = true
        await invalidateCurrentTransport(reason: reason)
        clearSessionContext()
        setState(.disconnected)
    }

    public func simulateNetworkDrop() async {
        guard state.isConnected, let transport else {
            return
        }

        wasUserInitiatedDisconnect = false
        await transport.close(reason: nil)
    }

    public func sendPointerMotion(
        x: Int32,
        y: Int32,
        sequence: UInt64
    ) async throws {
        guard state.isConnected, let transport else {
            throw SessionClientError.notConnected
        }

        try await transport.sendDatagram(
            InputPointerDatagram(sequence: sequence, x: x, y: y).encode()
        )
    }

    public func sendPointerButton(
        button: String,
        phase: String,
        x: Int32,
        y: Int32,
        sequence: UInt64
    ) async throws {
        guard state.isConnected, let transport else {
            throw SessionClientError.notConnected
        }

        try await transport.send(
            .pointerButton(
                button: button,
                phase: phase,
                x: x,
                y: y,
                sequence: sequence
            )
        )
    }

    public func sendWheel(
        deltaX: Int32,
        deltaY: Int32,
        x: Int32,
        y: Int32,
        sequence: UInt64
    ) async throws {
        guard state.isConnected, let transport else {
            throw SessionClientError.notConnected
        }

        try await transport.send(
            .pointerWheel(
                deltaX: deltaX,
                deltaY: deltaY,
                x: x,
                y: y,
                sequence: sequence
            )
        )
    }

    public func sendKey(
        keyCode: UInt16,
        phase: String,
        modifiers: UInt32
    ) async throws {
        guard state.isConnected, let transport else {
            throw SessionClientError.notConnected
        }

        try await transport.send(
            .keyboardKey(
                keyCode: keyCode,
                phase: phase,
                modifiers: modifiers
            )
        )
    }

    public func setInputFocus(active: Bool) async throws {
        guard state.isConnected, let transport else {
            throw SessionClientError.notConnected
        }

        try await transport.send(.inputFocus(active: active))
    }

    private func attemptResume(
        endpoint: SessionEndpoint,
        requestVideo: Bool
    ) async -> ResumeAttemptOutcome {
        guard let binding else {
            return .definitiveFailure("No resume token is available")
        }

        do {
            let preparedTransport = try await openTransport(endpoint: endpoint)
            try await preparedTransport.client.sendHello(
                clientName: "holobridge-avp",
                capabilities: capabilities(for: requestVideo)
            )
            _ = try await preparedTransport.client.awaitHelloAck()
            try await preparedTransport.client.sendResumeSession(resumeToken: binding.resumeToken)

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
            return .success(binding)
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
        self.binding = binding
        resumeTokenExpiry = Date().addingTimeInterval(TimeInterval(binding.resumeTokenTTLSeconds))
        wasUserInitiatedDisconnect = false
        setState(.connected(userDisplayName: binding.userDisplayName))
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

            do {
                for try await datagram in datagrams {
                    if Task.isCancelled {
                        return
                    }
                    await self.forwardVideoDatagram(datagram, generation: generation)
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

    private func forwardVideoDatagram(_ datagram: Data, generation: Int) {
        guard generation == transportGeneration else {
            return
        }
        onVideoDatagram?(datagram)
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
            wasUserInitiatedDisconnect = true
            await invalidateCurrentTransport(reason: nil)
            clearSessionContext()
            setState(.disconnected)
        case .pointerShape:
            onPointerShapeMessage?(message)
        default:
            break
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

        if wasUserInitiatedDisconnect {
            return
        }

        guard let endpoint = lastEndpoint, canAttemptResume(to: endpoint) else {
            setState(.error(error.localizedDescription))
            return
        }

        setState(.resuming)

        switch await attemptResume(endpoint: endpoint, requestVideo: requestVideo) {
        case .success:
            return
        case .definitiveFailure(let message):
            clearSessionContext(preserveEndpoint: true)
            setState(.error(message))
        case .transportFailure(let resumeError):
            setState(.error(resumeError.localizedDescription))
        }
    }

    private func handleVideoTransportTermination(
        _ error: Error,
        generation: Int
    ) async {
        guard generation == transportGeneration else {
            return
        }
        if case .connected = state {
            onStateChange?(.error("Video transport ended: \(error.localizedDescription)"))
        }
    }

    private func sessionBinding(
        from message: ControlMessage,
        action: String
    ) throws -> SessionBinding {
        guard let sessionID = message.sessionID, !sessionID.isEmpty else {
            throw SessionClientError.invalidSessionHandshake("\(action) result missing session_id")
        }
        guard let resumeToken = message.resumeToken, !resumeToken.isEmpty else {
            throw SessionClientError.invalidSessionHandshake("\(action) result missing resume_token")
        }
        guard let ttl = message.resumeTokenTTLSeconds, ttl > 0 else {
            throw SessionClientError.invalidSessionHandshake("\(action) result missing resume_token_ttl_secs")
        }

        return SessionBinding(
            sessionID: sessionID,
            resumeToken: resumeToken,
            resumeTokenTTLSeconds: ttl,
            userDisplayName: message.userDisplayName
        )
    }

    private func canAttemptResume(to endpoint: SessionEndpoint) -> Bool {
        guard
            let binding,
            !binding.sessionID.isEmpty,
            !binding.resumeToken.isEmpty,
            let expiry = resumeTokenExpiry,
            expiry > Date(),
            lastEndpoint == endpoint
        else {
            return false
        }
        return true
    }

    private func openTransport(endpoint: SessionEndpoint) async throws -> PreparedTransport {
        let config = transportConfigurationFactory(endpoint)
        let client = transportClientFactory(config)
        let generation = beginUsingTransport(client)

        do {
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
        binding = nil
        resumeTokenExpiry = nil
        wasUserInitiatedDisconnect = false
        if !preserveEndpoint {
            lastEndpoint = nil
        }
    }

    private func setState(_ newState: SessionClientState) {
        guard state != newState else {
            return
        }
        state = newState
        onStateChange?(newState)
    }

    private func capabilities(for requestVideo: Bool) -> [String] {
        guard requestVideo else {
            return [ControlMessage.controlStreamCapability]
        }

        return [
            ControlMessage.controlStreamCapability,
            ControlMessage.videoDatagramCapability,
            ControlMessage.pointerDatagramCapability,
            ControlMessage.pointerStreamCapability,
            ControlMessage.inputPointerDatagramCapability,
        ]
    }
}

public extension SessionClientState {
    var isConnected: Bool {
        if case .connected = self {
            return true
        }
        return false
    }
}
