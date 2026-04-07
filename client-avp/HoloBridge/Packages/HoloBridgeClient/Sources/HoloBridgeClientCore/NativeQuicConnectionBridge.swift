import Foundation
import Network

@available(macOS 26.0, iOS 26.0, visionOS 26.0, *)
final class NativeQuicConnectionBridge: QuicConnectionBridging {
    var onEvent: ((QuicBridgeEvent) -> Void)?
    var onControlPayload: ((Data) -> Void)?
    var onDatagramPayload: ((Data) -> Void)?
    var onTermination: ((Error?) -> Void)?

    private let configuration: TransportConfiguration
    private var connectionTask: Task<Void, Never>?
    private var controlSendContinuation: AsyncStream<SendRequest>.Continuation?
    private var datagramSendContinuation: AsyncStream<SendRequest>.Continuation?
    private var closeContinuation: CheckedContinuation<Void, Never>?
    private var didCompleteStart = false

    private struct SendRequest: Sendable {
        let payload: Data
        let completion: @Sendable (Error?) -> Void
    }

    init(configuration: TransportConfiguration, queueLabel: String) {
        self.configuration = configuration
    }

    func start(_ completion: @escaping (Error?) -> Void) {
        guard connectionTask == nil else {
            completion(NSError(
                domain: NSPOSIXErrorDomain,
                code: Int(EALREADY),
                userInfo: [NSLocalizedDescriptionKey: "QUIC transport is already started"]
            ))
            return
        }

        let (controlSendStream, controlSendCont) = AsyncStream.makeStream(of: SendRequest.self)
        let (datagramSendStream, datagramSendCont) = AsyncStream.makeStream(of: SendRequest.self)
        self.controlSendContinuation = controlSendCont
        self.datagramSendContinuation = datagramSendCont

        let config = configuration
        connectionTask = Task { [weak self] in
            do {
                guard let port = NWEndpoint.Port(rawValue: config.port) else {
                    throw TransportClientError.invalidConfiguration("invalid port: \(config.port)")
                }
                let endpoint = NWEndpoint.hostPort(
                    host: NWEndpoint.Host(config.host),
                    port: port
                )

                self?.emitEvent(.startingTransport)

                try await withNetworkConnection(to: endpoint, using: { [config] in
                    QUIC(alpn: [config.alpn])
                        .maxDatagramFrameSize(65_535)
                        .tls.peerAuthentication(.required)
                        .tls.certificateValidator { [config] _, _ in
                            config.allowInsecureCertificateValidation
                        }
                }) { [weak self] connection in
                    guard let self else { return }

                    self.emitEvent(.groupReady)

                    let stream = try await connection.openStream(directionality: .bidirectional)
                    self.emitEvent(.controlStreamReady)

                    let datagrams = try await connection.datagrams

                    self.didCompleteStart = true
                    completion(nil)

                    try await withThrowingTaskGroup(of: Void.self) { group in
                        // Receive control payloads
                        group.addTask { [weak self] in
                            while !Task.isCancelled {
                                let message = try await stream.receive(atLeast: 1, atMost: 65_536)
                                if let self {
                                    self.emitEvent(.controlPayloadReceived, detail: "\(message.content.count)")
                                    self.onControlPayload?(message.content)
                                }
                                if message.metadata.endOfStream { break }
                            }
                        }

                        // Receive datagrams
                        group.addTask { [weak self] in
                            while !Task.isCancelled {
                                let datagram = try await datagrams.receive()
                                if let self {
                                    self.emitEvent(.datagramReceived, detail: "\(datagram.content.count)")
                                    self.onDatagramPayload?(datagram.content)
                                }
                            }
                        }

                        // Send control payloads
                        group.addTask { [weak self] in
                            for await request in controlSendStream {
                                do {
                                    try await stream.send(request.payload, endOfStream: false)
                                    self?.emitEvent(.controlPayloadSent, detail: "\(request.payload.count)")
                                    request.completion(nil)
                                } catch {
                                    request.completion(error)
                                }
                            }
                        }

                        // Send datagrams
                        group.addTask {
                            for await request in datagramSendStream {
                                do {
                                    try await datagrams.send(request.payload)
                                    request.completion(nil)
                                } catch {
                                    request.completion(error)
                                }
                            }
                        }

                        // Close signal
                        group.addTask { [weak self] in
                            await withCheckedContinuation { continuation in
                                self?.closeContinuation = continuation
                            }
                        }

                        try await group.next()
                        group.cancelAll()
                    }
                }

                self?.emitEvent(.closeCompleted)
                self?.onTermination?(nil)
            } catch {
                guard let self else { return }
                if !self.didCompleteStart {
                    completion(error)
                }
                self.onTermination?(error)
            }
        }
    }

    func sendControlPayload(_ payload: Data, completion: @escaping (Error?) -> Void) {
        guard let cont = controlSendContinuation else {
            completion(NSError(
                domain: NSPOSIXErrorDomain,
                code: Int(ENOTCONN),
                userInfo: [NSLocalizedDescriptionKey: "control stream is not connected"]
            ))
            return
        }
        let sendableCompletion: @Sendable (Error?) -> Void = { error in completion(error) }
        cont.yield(SendRequest(payload: payload, completion: sendableCompletion))
    }

    func sendDatagramPayload(_ payload: Data, completion: @escaping (Error?) -> Void) {
        guard let cont = datagramSendContinuation else {
            completion(NSError(
                domain: NSPOSIXErrorDomain,
                code: Int(ENOTCONN),
                userInfo: [NSLocalizedDescriptionKey: "datagram channel is not connected"]
            ))
            return
        }
        let sendableCompletion: @Sendable (Error?) -> Void = { error in completion(error) }
        cont.yield(SendRequest(payload: payload, completion: sendableCompletion))
    }

    func close(reason: String?) {
        emitEvent(.closeInitiated, detail: reason)
        controlSendContinuation?.finish()
        datagramSendContinuation?.finish()
        closeContinuation?.resume(returning: ())
        connectionTask?.cancel()
    }

    private func emitEvent(_ kind: QuicBridgeEventKind, detail: String? = nil) {
        onEvent?(QuicBridgeEvent(kind: kind, detail: detail))
    }
}
