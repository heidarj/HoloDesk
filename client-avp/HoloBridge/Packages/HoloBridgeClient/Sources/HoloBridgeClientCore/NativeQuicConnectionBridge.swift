import Foundation
import Network
import os

final class NativeQuicConnectionBridge: QuicConnectionBridging, @unchecked Sendable {
    var onEvent: ((QuicBridgeEvent) -> Void)?
    var onControlPayload: ((Data) -> Void)?
    var onDatagramPayload: ((Data) -> Void)?
    var onTermination: ((Error?) -> Void)?

    private let configuration: TransportConfiguration
    private let logger = Logger(subsystem: "HoloBridge", category: "QuicBridge")
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

    deinit {
        logger.info("NativeQuicConnectionBridge deinit")
    }

    func start(_ completion: @escaping @Sendable (Error?) -> Void) {
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

                    try await withThrowingTaskGroup(of: String.self) { group in
                        // Receive control payloads (suspends forever after work)
                        group.addTask { [weak self] in
                            defer { self?.logger.info("task exiting: control-receive") }
                            do {
                                while !Task.isCancelled {
                                    let message = try await stream.receive(atLeast: 1, atMost: 65_536)
                                    if let self {
                                        self.emitEvent(.controlPayloadReceived, detail: "\(message.content.count)")
                                        self.onControlPayload?(message.content)
                                    }
                                    if message.metadata.endOfStream {
                                        self?.logger.info("control stream: server sent endOfStream")
                                        break
                                    }
                                }
                            } catch is CancellationError {
                                // Expected on close
                            } catch {
                                self?.logger.error("control stream read error: \(error.localizedDescription, privacy: .public)")
                                self?.onTermination?(error)
                            }
                            // Suspend until group is cancelled so this task
                            // does not trigger group.next() early.
                            await Task.yield()
                            while !Task.isCancelled {
                                try? await Task.sleep(for: .seconds(3600))
                            }
                            return "control-receive"
                        }

                        // Receive datagrams (suspends forever after work)
                        group.addTask { [weak self] in
                            defer { self?.logger.info("task exiting: datagram-receive") }
                            do {
                                while !Task.isCancelled {
                                    let datagram = try await datagrams.receive()
                                    if let self {
                                        self.emitEvent(.datagramReceived, detail: "\(datagram.content.count)")
                                        self.onDatagramPayload?(datagram.content)
                                    }
                                }
                            } catch is CancellationError {
                                // Expected on close
                            } catch {
                                self?.logger.error("datagram receive error: \(error.localizedDescription, privacy: .public)")
                                self?.onTermination?(error)
                            }
                            while !Task.isCancelled {
                                try? await Task.sleep(for: .seconds(3600))
                            }
                            return "datagram-receive"
                        }

                        // Send control payloads (suspends forever after stream ends)
                        group.addTask { [weak self] in
                            defer { self?.logger.info("task exiting: control-send") }
                            for await request in controlSendStream {
                                do {
                                    try await stream.send(request.payload, endOfStream: false)
                                    self?.emitEvent(.controlPayloadSent, detail: "\(request.payload.count)")
                                    request.completion(nil)
                                } catch {
                                    request.completion(error)
                                }
                            }
                            // Suspend until group is cancelled so this task
                            // does not trigger group.next() early.
                            while !Task.isCancelled {
                                try? await Task.sleep(for: .seconds(3600))
                            }
                            return "control-send"
                        }

                        // Send datagrams (suspends forever after stream ends)
                        group.addTask { [weak self] in
                            defer { self?.logger.info("task exiting: datagram-send") }
                            for await request in datagramSendStream {
                                do {
                                    try await datagrams.send(request.payload)
                                    request.completion(nil)
                                } catch {
                                    request.completion(error)
                                }
                            }
                            // Suspend until group is cancelled so this task
                            // does not trigger group.next() early.
                            while !Task.isCancelled {
                                try? await Task.sleep(for: .seconds(3600))
                            }
                            return "datagram-send"
                        }

                        // Close signal — only task that SHOULD trigger group.next()
                        group.addTask { [weak self] in
                            await withCheckedContinuation { continuation in
                                self?.closeContinuation = continuation
                            }
                            return "close-signal"
                        }

                        // Heartbeat — periodic log to confirm the connection scope is alive
                        group.addTask { [weak self] in
                            defer { self?.logger.info("task exiting: heartbeat") }
                            while !Task.isCancelled {
                                try? await Task.sleep(for: .seconds(5))
                                if !Task.isCancelled {
                                    self?.logger.info("bridge heartbeat: connection scope alive")
                                }
                            }
                            return "heartbeat"
                        }

                        let triggeredBy = try await group.next() ?? "nil"
                        self.logger.info("task group exiting, triggered by: \(triggeredBy, privacy: .public)")
                        group.cancelAll()
                    }
                }

                self?.logger.info("withNetworkConnection scope exited")
                self?.emitEvent(.closeCompleted)
                self?.logger.info("QUIC connection closed normally")
                self?.onTermination?(nil)
            } catch {
                guard let self else { return }
                self.logger.error("QUIC connection failed: \(error.localizedDescription, privacy: .public)")
                if !self.didCompleteStart {
                    completion(error)
                }
                self.onTermination?(error)
            }
        }
    }

    func sendControlPayload(_ payload: Data, completion: @escaping @Sendable (Error?) -> Void) {
        guard let cont = controlSendContinuation else {
            completion(NSError(
                domain: NSPOSIXErrorDomain,
                code: Int(ENOTCONN),
                userInfo: [NSLocalizedDescriptionKey: "control stream is not connected"]
            ))
            return
        }
        cont.yield(SendRequest(payload: payload, completion: completion))
    }

    func sendDatagramPayload(_ payload: Data, completion: @escaping @Sendable (Error?) -> Void) {
        guard let cont = datagramSendContinuation else {
            completion(NSError(
                domain: NSPOSIXErrorDomain,
                code: Int(ENOTCONN),
                userInfo: [NSLocalizedDescriptionKey: "datagram channel is not connected"]
            ))
            return
        }
        cont.yield(SendRequest(payload: payload, completion: completion))
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
