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
    public private(set) var streamWindowRequested = false
    public private(set) var remoteInputSuppressed = false

    @ObservationIgnored private let logger = Logger(subsystem: "HoloBridge", category: "Session")
    @ObservationIgnored private let videoPipeline: VideoStreamPipeline
    @ObservationIgnored private var sessionClient: SessionClient! = nil
    @ObservationIgnored private var connectTask: Task<Void, Never>?
    @ObservationIgnored private var streamWindowVisible = false
    @ObservationIgnored private var remoteInputFocusState = false
    @ObservationIgnored private var nextInputSequenceValue: UInt64 = 1

    #if DEBUG
    /// Creates a lightweight SessionManager for Xcode previews.
    /// No network transport is configured — `sessionClient` stays nil.
    init(preview state: SessionState) {
        let renderer = VideoRenderer()
        self.authMode = .apple
        self.videoRenderer = renderer
        self.videoPipeline = VideoStreamPipeline(renderer: renderer)
        self.state = state
        self.streamWindowRequested = state.isConnected
        renderer.updateFormat(width: 1920, height: 1080)
    }
    #endif

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
                        // Suppress per-datagram/per-payload noise
                        switch event.kind {
                        case .datagramReceived, .controlPayloadReceived, .controlPayloadSent:
                            return
                        default:
                            break
                        }
                        let detail = event.detail ?? "-"
                        logger.info("transport: \(event.kind.rawValue, privacy: .public) \(detail, privacy: .public)")
                    }
                )
            },
            onStateChange: { [weak self] newState in
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    self.logger.info("session state: \(String(describing: newState), privacy: .public)")
                    self.state = newState
                    switch newState {
                    case .connected:
                        self.streamWindowRequested = true
                        self.videoPipeline.prepareForStream()
                    case .disconnected, .error:
                        self.streamWindowRequested = false
                        self.videoPipeline.reset(statusMessage: "Waiting for stream")
                    case .connecting, .authenticating, .resuming:
                        self.videoPipeline.reset(statusMessage: "Waiting for stream")
                    }
                    self.synchronizeRemoteInputFocus()
                }
            },
            onVideoDatagram: { [weak self] datagram in
                Task { @MainActor [weak self] in
                    self?.videoPipeline.consume(datagram: datagram)
                }
            },
            onPointerShapeMessage: { [weak self] message in
                Task { @MainActor [weak self] in
                    self?.videoPipeline.consume(pointerShapeMessage: message)
                }
            }
        )
    }

    public func connect(host: String, port: UInt16) {
        connectTask?.cancel()
        connectTask = Task {
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
            } catch is CancellationError {
                logger.info("Connection cancelled by user")
                state = .disconnected
            } catch {
                logger.error("Connection failed: \(error.localizedDescription, privacy: .public)")
                if case .error = state {
                    return
                }
                state = .error(error.localizedDescription)
            }
        }
    }

    public func cancelConnection() async {
        connectTask?.cancel()
        connectTask = nil
        await sessionClient.disconnect(reason: "user-cancelled")
        state = .disconnected
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

    public func noteStreamWindowVisibility(_ isVisible: Bool) {
        streamWindowVisible = isVisible
        synchronizeRemoteInputFocus()
    }

    public func setOrnamentInteraction(active: Bool) {
        guard remoteInputSuppressed != active else {
            return
        }
        remoteInputSuppressed = active
        synchronizeRemoteInputFocus()
    }

    public func sendPointerMotion(
        x: Int32,
        y: Int32
    ) {
        guard canSendRemoteInput else {
            return
        }
        let sequence = nextInputSequence()
        Task {
            do {
                try await sessionClient.sendPointerMotion(x: x, y: y, sequence: sequence)
            } catch {
                logger.debug("pointer motion dropped: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    public func sendPointerButton(
        button: String,
        phase: String,
        x: Int32,
        y: Int32
    ) {
        guard canSendRemoteInput else {
            return
        }
        let sequence = nextInputSequence()
        Task {
            do {
                try await sessionClient.sendPointerButton(
                    button: button,
                    phase: phase,
                    x: x,
                    y: y,
                    sequence: sequence
                )
            } catch {
                logger.debug("pointer button dropped: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    public func sendWheel(
        deltaX: Int32,
        deltaY: Int32,
        x: Int32,
        y: Int32
    ) {
        guard canSendRemoteInput else {
            return
        }
        let sequence = nextInputSequence()
        Task {
            do {
                try await sessionClient.sendWheel(
                    deltaX: deltaX,
                    deltaY: deltaY,
                    x: x,
                    y: y,
                    sequence: sequence
                )
            } catch {
                logger.debug("pointer wheel dropped: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    public func sendKey(
        keyCode: UInt16,
        phase: String,
        modifiers: UInt32
    ) {
        guard canSendRemoteInput else {
            return
        }
        Task {
            do {
                try await sessionClient.sendKey(
                    keyCode: keyCode,
                    phase: phase,
                    modifiers: modifiers
                )
            } catch {
                logger.debug("keyboard input dropped: \(error.localizedDescription, privacy: .public)")
            }
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

    private var canSendRemoteInput: Bool {
        state.isConnected && streamWindowVisible && !remoteInputSuppressed
    }

    private func synchronizeRemoteInputFocus() {
        let desiredFocus = canSendRemoteInput
        guard desiredFocus != remoteInputFocusState else {
            return
        }

        remoteInputFocusState = desiredFocus
        guard state.isConnected else {
            return
        }

        Task {
            do {
                try await sessionClient.setInputFocus(active: desiredFocus)
            } catch {
                logger.debug("input focus update dropped: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    private func nextInputSequence() -> UInt64 {
        let sequence = nextInputSequenceValue
        nextInputSequenceValue &+= 1
        return sequence
    }
}
