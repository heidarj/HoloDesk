import Foundation

@available(visionOS 1.0, iOS 15.0, macOS 12.0, *)
public final class NetworkFrameworkQuicClient: TransportClient, @unchecked Sendable {
    public let configuration: TransportConfiguration

    private let queue: DispatchQueue
    private let diagnosticHandler: TransportDiagnosticHandler?
    private let bridgeFactory: @Sendable (TransportConfiguration, String) -> any QuicConnectionBridging

    private var bridge: (any QuicConnectionBridging)?
    private var framer = ControlMessageFramer()

    private var controlChunkStream: AsyncThrowingStream<Data, Error>?
    private var controlChunkContinuation: AsyncThrowingStream<Data, Error>.Continuation?
    private var controlChunkIterator: AsyncThrowingStream<Data, Error>.AsyncIterator?

    private var videoDatagramStream: AsyncThrowingStream<Data, Error>?
    private var videoDatagramContinuation: AsyncThrowingStream<Data, Error>.Continuation?

    public init(
        configuration: TransportConfiguration,
        queue: DispatchQueue = DispatchQueue(label: "HoloBridge.Transport.NetworkFrameworkQuicClient"),
        diagnosticHandler: TransportDiagnosticHandler? = nil
    ) {
        self.configuration = configuration
        self.queue = queue
        self.diagnosticHandler = diagnosticHandler
        self.bridgeFactory = { configuration, queueLabel in
            if #available(macOS 26.0, iOS 26.0, visionOS 26.0, *) {
                return NativeQuicConnectionBridge(configuration: configuration, queueLabel: queueLabel)
            }
            return ObjectiveCQuicConnectionBridgeAdapter(configuration: configuration, queueLabel: queueLabel)
        }
    }

    init(
        configuration: TransportConfiguration,
        queue: DispatchQueue = DispatchQueue(label: "HoloBridge.Transport.NetworkFrameworkQuicClient"),
        diagnosticHandler: TransportDiagnosticHandler? = nil,
        bridgeFactory: @escaping @Sendable (TransportConfiguration, String) -> any QuicConnectionBridging
    ) {
        self.configuration = configuration
        self.queue = queue
        self.diagnosticHandler = diagnosticHandler
        self.bridgeFactory = bridgeFactory
    }

    deinit {
        bridge?.close(reason: nil)
        finishControlChunks(error: nil)
        finishVideoDatagrams(error: nil)
    }

    public func connect() async throws {
        guard bridge == nil else {
            return
        }

        let bridge = bridgeFactory(configuration, queue.label)
        self.bridge = bridge
        prepareStreams()
        wireBridgeCallbacks(bridge)

        do {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                bridge.start { error in
                    if let error {
                        continuation.resume(throwing: self.wrapBridgeError(error))
                    } else {
                        continuation.resume(returning: ())
                    }
                }
            }
        } catch {
            self.bridge = nil
            finishControlChunks(error: error)
            finishVideoDatagrams(error: error)
            throw error
        }
    }

    public func armVideoDatagramReceive() -> AsyncThrowingStream<Data, Error> {
        if let videoDatagramStream {
            return videoDatagramStream
        }

        let stream = AsyncThrowingStream<Data, Error> { continuation in
            self.videoDatagramContinuation = continuation
            continuation.onTermination = { [weak self] _ in
                self?.videoDatagramContinuation = nil
            }
        }
        videoDatagramStream = stream
        return stream
    }

    public func send(_ message: ControlMessage) async throws {
        guard let bridge else {
            throw TransportClientError.notConnected
        }

        let frame: Data
        do {
            frame = try ControlMessageCodec.encodeFrame(message)
        } catch let error as ControlMessageCodecError {
            throw TransportClientError.codec(error)
        }

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            bridge.sendControlPayload(frame) { error in
                if let error {
                    continuation.resume(throwing: self.wrapBridgeError(error))
                } else {
                    continuation.resume(returning: ())
                }
            }
        }
    }

    public func sendHello(clientName: String, capabilities: [String]) async throws {
        try await send(.hello(clientName: clientName, capabilities: capabilities))
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
        if let reason, bridge != nil {
            try? await send(.goodbye(reason: reason))
        }

        bridge?.close(reason: reason)
        bridge = nil
        framer.reset()
        finishControlChunks(error: nil)
        finishVideoDatagrams(error: nil)
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
        guard var iterator = controlChunkIterator else {
            throw TransportClientError.notConnected
        }

        do {
            if let chunk = try await iterator.next() {
                controlChunkIterator = iterator
                return chunk
            }
            controlChunkIterator = iterator
            throw TransportClientError.connectionClosed
        } catch {
            controlChunkIterator = iterator
            throw error
        }
    }

    private func prepareStreams() {
        let controlStream = AsyncThrowingStream<Data, Error> { continuation in
            self.controlChunkContinuation = continuation
            continuation.onTermination = { [weak self] _ in
                self?.controlChunkContinuation = nil
            }
        }
        controlChunkStream = controlStream
        controlChunkIterator = controlStream.makeAsyncIterator()

        if videoDatagramStream == nil {
            let datagramStream = AsyncThrowingStream<Data, Error> { continuation in
                self.videoDatagramContinuation = continuation
                continuation.onTermination = { [weak self] _ in
                    self?.videoDatagramContinuation = nil
                }
            }
            videoDatagramStream = datagramStream
        }
    }

    private func wireBridgeCallbacks(_ bridge: any QuicConnectionBridging) {
        bridge.onEvent = { [weak self] event in
            self?.emitDiagnostic(event)
        }
        bridge.onControlPayload = { [weak self] payload in
            self?.controlChunkContinuation?.yield(payload)
        }
        bridge.onDatagramPayload = { [weak self] payload in
            self?.videoDatagramContinuation?.yield(payload)
        }
        bridge.onTermination = { [weak self] error in
            guard let self else { return }
            let wrapped = error.map(self.wrapBridgeError)
            self.bridge = nil
            self.finishControlChunks(error: wrapped)
            self.finishVideoDatagrams(error: wrapped)
        }
    }

    private func finishControlChunks(error: Error?) {
        if let error {
            controlChunkContinuation?.finish(throwing: error)
        } else {
            controlChunkContinuation?.finish()
        }
        controlChunkContinuation = nil
        controlChunkStream = nil
        controlChunkIterator = nil
    }

    private func finishVideoDatagrams(error: Error?) {
        if let error {
            videoDatagramContinuation?.finish(throwing: error)
        } else {
            videoDatagramContinuation?.finish()
        }
        videoDatagramContinuation = nil
        videoDatagramStream = nil
    }

    private func emitDiagnostic(_ event: QuicBridgeEvent) {
        guard let diagnosticHandler else {
            return
        }

        let kind: TransportDiagnosticEvent.Kind
        switch event.kind {
        case .startingTransport:
            kind = .startingTransport
        case .groupReady:
            kind = .groupReady
        case .groupFailed:
            kind = .groupFailed
        case .groupCancelled:
            kind = .groupCancelled
        case .controlStreamExtracted:
            kind = .controlStreamExtracted
        case .controlStreamReady:
            kind = .controlStreamReady
        case .controlStreamFailed:
            kind = .controlStreamFailed
        case .controlStreamCancelled:
            kind = .controlStreamCancelled
        case .controlPayloadSent:
            kind = .controlPayloadSent
        case .controlPayloadReceived:
            kind = .controlPayloadReceived
        case .datagramReceived:
            kind = .datagramReceived
        case .closeInitiated:
            kind = .closeInitiated
        case .closeCompleted:
            kind = .closeCompleted
        }

        diagnosticHandler(TransportDiagnosticEvent(kind: kind, detail: event.detail))
    }

    private func wrapBridgeError(_ error: Error) -> Error {
        if let transportError = error as? TransportClientError {
            return transportError
        }
        return TransportClientError.connectionFailed(error.localizedDescription)
    }
}
