import Foundation
import HoloBridgeClientQuicBridge
import Network
import Security

private let streamPing = Data("hb-stream-ping-v1".utf8)
private let streamAck = Data("hb-stream-ack-v1".utf8)
private let datagramPing = Data("hb-datagram-ping-v1".utf8)
private let datagramAck = Data("hb-datagram-ack-v1".utf8)

@available(macOS 12.0, *)
private enum InteropMode: String {
    case stream
    case datagram
    case mixed
}

@available(macOS 12.0, *)
private struct InteropOptions {
    var mode: InteropMode = .stream
    var host = "127.0.0.1"
    var port: UInt16 = 4433
    var alpn = "holobridge-m2"
    var allowInsecureCert = false
    var timeoutSeconds: TimeInterval = 5

    static func parse(_ arguments: ArraySlice<String>) throws -> InteropOptions {
        var options = InteropOptions()
        var iterator = arguments.makeIterator()

        while let argument = iterator.next() {
            switch argument {
            case "--mode":
                let value = try nextValue("--mode", &iterator)
                guard let mode = InteropMode(rawValue: value) else {
                    throw InteropError("unsupported mode: \(value)")
                }
                options.mode = mode
            case "--host":
                options.host = try nextValue("--host", &iterator)
            case "--port":
                let value = try nextValue("--port", &iterator)
                guard let port = UInt16(value) else {
                    throw InteropError("--port requires a valid UInt16")
                }
                options.port = port
            case "--alpn":
                options.alpn = try nextValue("--alpn", &iterator)
            case "--allow-insecure-cert":
                options.allowInsecureCert = true
            case "--timeout-seconds":
                let value = try nextValue("--timeout-seconds", &iterator)
                guard let timeout = TimeInterval(value) else {
                    throw InteropError("--timeout-seconds requires a valid number")
                }
                options.timeoutSeconds = timeout
            case "--help", "-h":
                throw InteropUsage()
            default:
                throw InteropError("unknown argument: \(argument)")
            }
        }

        return options
    }

    static func usage() -> String {
        """
        usage: holobridge-quic-interop-smoke [--mode stream|datagram|mixed] [--host value] [--port value] [--alpn value] [--allow-insecure-cert] [--timeout-seconds value]
        """
    }

    private static func nextValue(
        _ flag: String,
        _ iterator: inout ArraySlice<String>.Iterator
    ) throws -> String {
        guard let value = iterator.next() else {
            throw InteropError("\(flag) requires a value")
        }
        return value
    }
}

private struct InteropUsage: Error {}
private struct InteropError: LocalizedError {
    let detail: String
    init(_ detail: String) { self.detail = detail }
    var errorDescription: String? { detail }
}

private struct TimeoutError: LocalizedError {
    let operation: String
    var errorDescription: String? { "timed out waiting for \(operation)" }
}

@available(macOS 12.0, *)
private final class BridgeHarness {
    let bridge: HBQuicConnectionBridge
    private let controlBuffer = PayloadBuffer(endMessage: "control stream ended before ack")
    private let datagramBuffer = PayloadBuffer(endMessage: "datagram stream ended before ack")

    init(options: InteropOptions, mode: HBQuicBridgeMode) {
        self.bridge = HBQuicConnectionBridge(
            host: options.host,
            port: options.port,
            serverName: "localhost",
            alpn: options.alpn,
            allowInsecureCertAuth: options.allowInsecureCert,
            pinnedCertificateFingerprint: nil,
            queueLabel: "HoloBridge.Interop.\(mode.rawValue)",
            mode: mode
        )

        bridge.eventHandler = { eventType, detail in
            print(formatBridgeEvent(eventType, detail: detail))
        }
        bridge.controlPayloadHandler = { [controlBuffer] payload in
            Task {
                await controlBuffer.push(payload)
            }
        }
        bridge.datagramHandler = { [datagramBuffer] payload in
            Task {
                await datagramBuffer.push(payload)
            }
        }
        bridge.terminationHandler = { [controlBuffer, datagramBuffer] error in
            Task {
                await controlBuffer.finish(error)
                await datagramBuffer.finish(error)
            }
        }
    }

    func start() async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            bridge.start { error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume(returning: ())
                }
            }
        }
    }

    func sendControl(_ payload: Data) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            bridge.sendControlPayload(payload) { error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume(returning: ())
                }
            }
        }
    }

    func sendDatagram(_ payload: Data) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            bridge.sendDatagramPayload(payload) { error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume(returning: ())
                }
            }
        }
    }

    func close() {
        bridge.close(withReason: "interop-complete")
    }

    func nextControl() async throws -> Data {
        try await controlBuffer.next()
    }

    func nextDatagram() async throws -> Data {
        try await datagramBuffer.next()
    }
}

@available(macOS 12.0, *)
private actor PayloadBuffer {
    private let endMessage: String
    private var buffered: [Data] = []
    private var waiters: [CheckedContinuation<Data, Error>] = []
    private var terminalError: Error?
    private var finished = false

    init(endMessage: String) {
        self.endMessage = endMessage
    }

    func push(_ payload: Data) {
        if let waiter = waiters.first {
            waiters.removeFirst()
            waiter.resume(returning: payload)
        } else {
            buffered.append(payload)
        }
    }

    func finish(_ error: Error?) {
        if let error {
            terminalError = error
            for waiter in waiters {
                waiter.resume(throwing: error)
            }
        } else {
            finished = true
            for waiter in waiters {
                waiter.resume(throwing: InteropError(endMessage))
            }
        }
        waiters.removeAll()
    }

    func next() async throws -> Data {
        if !buffered.isEmpty {
            return buffered.removeFirst()
        }
        if let terminalError {
            throw terminalError
        }
        if finished {
            throw InteropError(endMessage)
        }

        return try await withCheckedThrowingContinuation { continuation in
            waiters.append(continuation)
        }
    }
}

@available(macOS 12.0, *)
@main
enum InteropMain {
    static func main() async {
        do {
            let options = try InteropOptions.parse(CommandLine.arguments.dropFirst())
            try await run(options: options)
        } catch is InteropUsage {
            print(InteropOptions.usage())
            exit(EXIT_SUCCESS)
        } catch {
            fputs("error: \(error.localizedDescription)\n", stderr)
            fputs("\(InteropOptions.usage())\n", stderr)
            exit(EXIT_FAILURE)
        }
    }

    private static func run(options: InteropOptions) async throws {
        switch options.mode {
        case .stream:
            try await runStreamOnly(options: options)
        case .datagram:
            try await runDatagramOnly(options: options)
        case .mixed:
            try await runMixed(options: options)
        }
    }

    private static func runStreamOnly(options: InteropOptions) async throws {
        let connection = try makeStreamConnection(options: options)
        defer { connection.cancel() }

        try await waitForConnectionReady(connection)
        print("stream_event: ready")

        try await send(connection: connection, payload: streamPing)
        print("stream_event: payload_sent size=\(streamPing.count)")

        let response = try await withTimeout(
            seconds: options.timeoutSeconds,
            operation: "stream response"
        ) {
            try await receive(connection: connection)
        }

        guard response == streamAck else {
            throw InteropError("unexpected stream ack: \(response as NSData)")
        }

        print("mode: stream")
        print("result: success")
    }

    private static func runDatagramOnly(options: InteropOptions) async throws {
        guard #available(macOS 26.0, iOS 26.0, visionOS 26.0, *) else {
            throw InteropError("datagram mode requires macOS 26.0+ (native QUIC datagram API)")
        }
        try await runDatagramOnlyNative(options: options)
    }

    @available(macOS 26.0, iOS 26.0, visionOS 26.0, *)
    private static func runDatagramOnlyNative(options: InteropOptions) async throws {
        guard let port = NWEndpoint.Port(rawValue: options.port) else {
            throw InteropError("invalid port: \(options.port)")
        }
        let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(options.host), port: port)

        try await withTimeout(seconds: options.timeoutSeconds, operation: "datagram exchange") {
            try await withNetworkConnection(to: endpoint, using: {
                QUIC(alpn: [options.alpn])
                    .maxDatagramFrameSize(65_535)
                    .tls.peerAuthentication(.required)
                    .tls.certificateValidator { _, _ in true }
            }) { connection in
                print("mode: datagram")
                print("datagram_event: connected")

                let datagrams = try await connection.datagrams
                print("datagram_event: datagram_channel_ready")

                try await datagrams.send(datagramPing)
                print("datagram_event: sent_ping size=\(datagramPing.count)")

                let reply = try await datagrams.receive()
                guard reply.content == datagramAck else {
                    throw InteropError("unexpected datagram response: \(reply.content as NSData)")
                }
                print("datagram_event: received_ack size=\(reply.content.count)")
                print("result: success")
            }
        }
    }

    private static func runMixed(options: InteropOptions) async throws {
        guard #available(macOS 26.0, iOS 26.0, visionOS 26.0, *) else {
            throw InteropError("mixed mode requires macOS 26.0+ (native QUIC datagram API)")
        }
        try await runMixedNative(options: options)
    }

    @available(macOS 26.0, iOS 26.0, visionOS 26.0, *)
    private static func runMixedNative(options: InteropOptions) async throws {
        guard let port = NWEndpoint.Port(rawValue: options.port) else {
            throw InteropError("invalid port: \(options.port)")
        }
        let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(options.host), port: port)

        try await withTimeout(seconds: options.timeoutSeconds, operation: "mixed exchange") {
            try await withNetworkConnection(to: endpoint, using: {
                QUIC(alpn: [options.alpn])
                    .maxDatagramFrameSize(65_535)
                    .tls.peerAuthentication(.required)
                    .tls.certificateValidator { _, _ in true }
            }) { connection in
                print("mode: mixed")

                // Stream test
                let stream = try await connection.openStream(directionality: .bidirectional)
                try await stream.send(streamPing, endOfStream: true)
                print("mixed_event: stream_sent size=\(streamPing.count)")

                var streamReply = Data()
                while true {
                    let message = try await stream.receive(atLeast: 1, atMost: 65_536)
                    streamReply.append(message.content)
                    if message.metadata.endOfStream { break }
                }
                guard streamReply == streamAck else {
                    throw InteropError("unexpected stream response: \(streamReply as NSData)")
                }
                print("mixed_event: stream_ack_received size=\(streamReply.count)")

                // Datagram test
                let datagrams = try await connection.datagrams
                try await datagrams.send(datagramPing)
                print("mixed_event: datagram_sent size=\(datagramPing.count)")

                let datagramReply = try await datagrams.receive()
                guard datagramReply.content == datagramAck else {
                    throw InteropError("unexpected datagram response: \(datagramReply.content as NSData)")
                }
                print("mixed_event: datagram_ack_received size=\(datagramReply.content.count)")
                print("result: success")
            }
        }
    }
}

@available(macOS 12.0, *)
private func makeStreamConnection(options: InteropOptions) throws -> NWConnection {
    guard let port = NWEndpoint.Port(rawValue: options.port) else {
        throw InteropError("invalid port: \(options.port)")
    }

    return NWConnection(
        host: NWEndpoint.Host(options.host),
        port: port,
        using: try makeQuicParameters(options: options)
    )
}

@available(macOS 12.0, *)
private func makeQuicParameters(options: InteropOptions) throws -> NWParameters {
    let quicOptions = NWProtocolQUIC.Options()
    sec_protocol_options_add_tls_application_protocol(
        quicOptions.securityProtocolOptions,
        options.alpn
    )

    sec_protocol_options_set_tls_server_name(
        quicOptions.securityProtocolOptions,
        "localhost"
    )

    if options.allowInsecureCert {
        let queue = DispatchQueue(label: "HoloBridge.Interop.StreamVerify")
        sec_protocol_options_set_verify_block(
            quicOptions.securityProtocolOptions,
            { _, _, completion in
                completion(true)
            },
            queue
        )
    }

    let parameters = NWParameters(quic: quicOptions)
    parameters.allowLocalEndpointReuse = true
    return parameters
}

@available(macOS 12.0, *)
private func waitForConnectionReady(_ connection: NWConnection) async throws {
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        final class Gate: @unchecked Sendable {
            var resumed = false
        }

        let gate = Gate()
        connection.stateUpdateHandler = { state in
            guard !gate.resumed else {
                return
            }

            switch state {
            case .ready:
                gate.resumed = true
                continuation.resume(returning: ())
            case .failed(let error):
                gate.resumed = true
                continuation.resume(throwing: error)
            case .cancelled:
                gate.resumed = true
                continuation.resume(throwing: InteropError("stream connection was cancelled"))
            default:
                break
            }
        }

        connection.start(queue: DispatchQueue(label: "HoloBridge.Interop.StreamConnection"))
    }
}

@available(macOS 12.0, *)
private func send(connection: NWConnection, payload: Data) async throws {
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        connection.send(content: payload, completion: .contentProcessed { error in
            if let error {
                continuation.resume(throwing: error)
            } else {
                continuation.resume(returning: ())
            }
        })
    }
}

@available(macOS 12.0, *)
private func receive(connection: NWConnection) async throws -> Data {
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
        connection.receive(minimumIncompleteLength: 1, maximumLength: 4096) { content, _, isComplete, error in
            if let error {
                continuation.resume(throwing: error)
                return
            }

            let payload = content ?? Data()
            if isComplete && payload.isEmpty {
                continuation.resume(throwing: InteropError("stream connection finished before ack"))
                return
            }

            continuation.resume(returning: payload)
        }
    }
}

private func withTimeout<T: Sendable>(
    seconds: TimeInterval,
    operation: String,
    _ body: @escaping @Sendable () async throws -> T
) async throws -> T {
    try await withThrowingTaskGroup(of: T.self) { group in
        group.addTask {
            try await body()
        }
        group.addTask {
            let duration = UInt64(seconds * 1_000_000_000)
            try await Task.sleep(nanoseconds: duration)
            throw TimeoutError(operation: operation)
        }

        let result = try await group.next()!
        group.cancelAll()
        return result
    }
}

private func formatBridgeEvent(
    _ eventType: HBQuicBridgeEventType,
    detail: String?
) -> String {
    let kind: String
    switch eventType {
    case .startingTransport:
        kind = "startingTransport"
    case .groupReady:
        kind = "groupReady"
    case .groupFailed:
        kind = "groupFailed"
    case .groupCancelled:
        kind = "groupCancelled"
    case .controlStreamExtracted:
        kind = "controlStreamExtracted"
    case .controlStreamReady:
        kind = "controlStreamReady"
    case .controlStreamFailed:
        kind = "controlStreamFailed"
    case .controlStreamCancelled:
        kind = "controlStreamCancelled"
    case .controlPayloadSent:
        kind = "controlPayloadSent"
    case .controlPayloadReceived:
        kind = "controlPayloadReceived"
    case .datagramReceived:
        kind = "datagramReceived"
    case .closeInitiated:
        kind = "closeInitiated"
    case .closeCompleted:
        kind = "closeCompleted"
    @unknown default:
        kind = "unknown"
    }

    if let detail, !detail.isEmpty {
        return "bridge_event: \(kind) detail=\(detail)"
    }
    return "bridge_event: \(kind)"
}
