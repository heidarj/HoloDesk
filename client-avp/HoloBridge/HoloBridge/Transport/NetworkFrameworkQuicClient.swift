import CryptoKit
import Foundation
import Network
import Security

@available(visionOS 1.0, iOS 15.0, macOS 12.0, *)
public final class NetworkFrameworkQuicClient: TransportClient, @unchecked Sendable {
    public let configuration: TransportConfiguration

    private let queue: DispatchQueue
    private var connectionGroup: NWConnectionGroup?
    private var controlConnection: NWConnection?
    private var framer = ControlMessageFramer()
    private var videoDatagramStream: AsyncThrowingStream<Data, Error>?
    private var videoDatagramContinuation: AsyncThrowingStream<Data, Error>.Continuation?

    private final class ResumeGate: @unchecked Sendable {
        private let lock = NSLock()
        private var resumed = false

        nonisolated func claim() -> Bool {
            lock.lock()
            defer { lock.unlock() }

            guard !resumed else {
                return false
            }
            resumed = true
            return true
        }
    }

    public init(
        configuration: TransportConfiguration,
        queue: DispatchQueue = DispatchQueue(label: "HoloBridge.Transport.NetworkFrameworkQuicClient")
    ) {
        self.configuration = configuration
        self.queue = queue
    }

    deinit {
        controlConnection?.cancel()
        connectionGroup?.cancel()
        finishVideoDatagrams(error: nil)
    }

    public func connect() async throws {
        guard controlConnection == nil, connectionGroup == nil else {
            return
        }

        guard let port = NWEndpoint.Port(rawValue: configuration.port) else {
            throw TransportClientError.invalidConfiguration("port \(configuration.port) is not a valid NWEndpoint.Port")
        }

        let parameters = try Self.makeParameters(configuration: configuration, queue: queue)
        let descriptor = NWMultiplexGroup(
            to: .hostPort(
                host: NWEndpoint.Host(configuration.host),
                port: port
            )
        )
        let group = NWConnectionGroup(with: descriptor, using: parameters)
        self.connectionGroup = group

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let gate = ResumeGate()

            group.stateUpdateHandler = { [weak self] state in
                Task { @MainActor [weak self] in
                    self?.handleGroupStateChange(state)
                }

                switch state {
                case .ready:
                    guard gate.claim() else { return }
                    continuation.resume(returning: ())
                case .failed(let error):
                    guard gate.claim() else { return }
                    continuation.resume(throwing: TransportClientError.connectionFailed(String(describing: error)))
                case .cancelled:
                    guard gate.claim() else { return }
                    continuation.resume(throwing: TransportClientError.connectionClosed)
                default:
                    break
                }
            }

            group.setReceiveHandler(
                maximumMessageSize: 65_536,
                rejectOversizedMessages: true
            ) { [weak self] _, content, isComplete in
                Task { @MainActor [weak self] in
                    self?.handleMediaDatagram(content: content, isComplete: isComplete)
                }
            }

            group.start(queue: self.queue)
        }

        guard let controlConnection = NWConnection(from: group) else {
            throw TransportClientError.connectionFailed("failed to derive a control connection from the QUIC connection group")
        }
        self.controlConnection = controlConnection

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let gate = ResumeGate()

            controlConnection.stateUpdateHandler = { [weak self] state in
                Task { @MainActor [weak self] in
                    self?.handleControlStateChange(state)
                }

                switch state {
                case .ready:
                    guard gate.claim() else { return }
                    continuation.resume(returning: ())
                case .failed(let error):
                    guard gate.claim() else { return }
                    continuation.resume(throwing: TransportClientError.connectionFailed(String(describing: error)))
                case .cancelled:
                    guard gate.claim() else { return }
                    continuation.resume(throwing: TransportClientError.connectionClosed)
                default:
                    break
                }
            }

            controlConnection.start(queue: self.queue)
        }
    }

    public func armVideoDatagramReceive() -> AsyncThrowingStream<Data, Error> {
        if let videoDatagramStream {
            return videoDatagramStream
        }

        let stream = AsyncThrowingStream<Data, Error> { continuation in
            self.videoDatagramContinuation = continuation
            continuation.onTermination = { [weak self] _ in
                Task { @MainActor in
                    self?.clearVideoDatagramContinuation()
                }
            }
        }

        videoDatagramStream = stream
        return stream
    }

    public func send(_ message: ControlMessage) async throws {
        guard let controlConnection else {
            throw TransportClientError.notConnected
        }

        do {
            let frame = try ControlMessageCodec.encodeFrame(message)
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                controlConnection.send(content: frame, completion: .contentProcessed { error in
                    if let error {
                        continuation.resume(throwing: TransportClientError.connectionFailed(String(describing: error)))
                    } else {
                        continuation.resume()
                    }
                })
            }
        } catch let error as ControlMessageCodecError {
            throw TransportClientError.codec(error)
        }
    }

    public func sendHello(
        clientName: String,
        capabilities: [String]
    ) async throws {
        try await send(ControlMessage.hello(clientName: clientName, capabilities: capabilities))
    }

    public func receive() async throws -> ControlMessage {
        try await receiveMessage()
    }

    public func awaitHelloAck() async throws -> ControlMessage {
        let message = try await receive()
        guard message.type == .helloAck else {
            throw TransportClientError.unexpectedMessage(message.kind)
        }
        return message
    }

    public func awaitAuthResult() async throws -> ControlMessage {
        let message = try await receive()
        guard message.type == .authResult else {
            throw TransportClientError.unexpectedMessage(message.kind)
        }
        return message
    }

    public func awaitResumeResult() async throws -> ControlMessage {
        let message = try await receive()
        guard message.type == .resumeResult else {
            throw TransportClientError.unexpectedMessage(message.kind)
        }
        return message
    }

    public func close(reason: String?) async {
        if let reason, controlConnection != nil {
            try? await send(.goodbye(reason: reason))
        }

        controlConnection?.cancel()
        controlConnection = nil
        connectionGroup?.cancel()
        connectionGroup = nil
        framer.reset()
        finishVideoDatagrams(error: nil)
        videoDatagramStream = nil
    }

    private func receiveMessage() async throws -> ControlMessage {
        while true {
            do {
                if let message = try framer.nextMessage() {
                    return message
                }
            } catch let error as ControlMessageCodecError {
                throw TransportClientError.codec(error)
            }

            let chunk = try await receiveChunk()
            framer.append(chunk)
        }
    }

    private func receiveChunk() async throws -> Data {
        guard let controlConnection else {
            throw TransportClientError.notConnected
        }

        return try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
            controlConnection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { content, _, isComplete, error in
                if let error {
                    continuation.resume(throwing: TransportClientError.connectionFailed(String(describing: error)))
                    return
                }

                let data = content ?? Data()
                if isComplete, data.isEmpty {
                    continuation.resume(throwing: TransportClientError.connectionClosed)
                } else {
                    continuation.resume(returning: data)
                }
            }
        }
    }

    private func handleGroupStateChange(_ state: NWConnectionGroup.State) {
        switch state {
        case .failed(let error):
            finishVideoDatagrams(error: TransportClientError.connectionFailed(String(describing: error)))
        case .cancelled:
            finishVideoDatagrams(error: nil)
        default:
            break
        }
    }

    private func handleControlStateChange(_ state: NWConnection.State) {
        switch state {
        case .failed(let error):
            finishVideoDatagrams(error: TransportClientError.connectionFailed(String(describing: error)))
        case .cancelled:
            finishVideoDatagrams(error: nil)
        default:
            break
        }
    }

    private func handleMediaDatagram(
        content: Data?,
        isComplete: Bool
    ) {
        guard let content, !content.isEmpty else {
            if isComplete {
                finishVideoDatagrams(error: nil)
            }
            return
        }

        videoDatagramContinuation?.yield(content)
    }

    private func finishVideoDatagrams(error: Error?) {
        if let error {
            videoDatagramContinuation?.finish(throwing: error)
        } else {
            videoDatagramContinuation?.finish()
        }
        videoDatagramContinuation = nil
    }

    private func clearVideoDatagramContinuation() {
        videoDatagramContinuation = nil
    }

    private static func makeParameters(
        configuration: TransportConfiguration,
        queue: DispatchQueue
    ) throws -> NWParameters {
        let quicOptions = NWProtocolQUIC.Options()
        quicOptions.direction = .bidirectional
        quicOptions.isDatagram = true
        quicOptions.maxDatagramFrameSize = 65_535

        sec_protocol_options_add_tls_application_protocol(
            quicOptions.securityProtocolOptions,
            configuration.alpn
        )

        let serverName = configuration.serverName ?? configuration.host
        sec_protocol_options_set_tls_server_name(
            quicOptions.securityProtocolOptions,
            serverName
        )

        if configuration.allowInsecureCertificateValidation || configuration.pinnedServerCertificateSHA256 != nil {
            sec_protocol_options_set_verify_block(
                quicOptions.securityProtocolOptions,
                { _, trust, completion in
                    if configuration.allowInsecureCertificateValidation {
                        completion(true)
                        return
                    }

                    guard let expectedFingerprint = configuration.pinnedServerCertificateSHA256 else {
                        completion(false)
                        return
                    }

                    let trustRef = sec_trust_copy_ref(trust).takeRetainedValue()
                    guard let certificate = copyLeafCertificate(from: trustRef) else {
                        completion(false)
                        return
                    }

                    let certificateData = SecCertificateCopyData(certificate) as Data
                    let digest = SHA256.hash(data: certificateData)
                    let actualFingerprint = digest.map { String(format: "%02x", $0) }.joined()
                    completion(normalizeFingerprint(actualFingerprint) == normalizeFingerprint(expectedFingerprint))
                },
                queue
            )
        }

        let parameters = NWParameters(quic: quicOptions)
        parameters.allowLocalEndpointReuse = true
        return parameters
    }

    private static func normalizeFingerprint(_ value: String) -> String {
        value.replacingOccurrences(of: ":", with: "").trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    private static func copyLeafCertificate(from trust: SecTrust) -> SecCertificate? {
        (SecTrustCopyCertificateChain(trust) as? [SecCertificate])?.first
    }
}
