import Foundation
@testable import HoloBridgeClientCore
import XCTest

final class SessionClientTests: XCTestCase {
    func testSuccessfulHelloAuthPath() async throws {
        let transport = MockTransportClient()
        transport.enqueueIncoming(.helloAck())
        transport.enqueueIncoming(
            .authResult(
                success: true,
                message: "authenticated",
                userDisplayName: "Test User",
                sessionID: "session-1",
                resumeToken: "resume-1",
                resumeTokenTTLSeconds: 3600
            )
        )

        let client = SessionClient(transportClientFactory: { _ in transport })

        let binding = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token" },
            requestVideo: true
        )

        XCTAssertEqual(binding.sessionID, "session-1")
        let sent = transport.sentMessages()
        XCTAssertEqual(sent.first?.type, .hello)
        XCTAssertEqual(sent.last?.type, .authenticate)
    }

    func testResumeBeforeAuthWhenCachedTokenIsValid() async throws {
        let first = MockTransportClient()
        first.enqueueIncoming(.helloAck())
        first.enqueueIncoming(
            .authResult(
                success: true,
                message: "authenticated",
                sessionID: "session-1",
                resumeToken: "resume-1",
                resumeTokenTTLSeconds: 3600
            )
        )

        let second = MockTransportClient()
        second.enqueueIncoming(.helloAck())
        second.enqueueIncoming(
            .resumeResult(
                success: true,
                message: "resumed",
                sessionID: "session-1",
                resumeToken: "resume-2",
                resumeTokenTTLSeconds: 3600
            )
        )

        let factory = MockTransportFactory(transports: [first, second])
        let client = SessionClient(transportClientFactory: { configuration in
            factory.makeTransport(configuration: configuration)
        })

        _ = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token" },
            requestVideo: false
        )
        _ = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token" },
            requestVideo: false
        )

        XCTAssertEqual(second.sentMessages().map(\.type), [.hello, .resumeSession])
    }

    func testDefinitiveResumeFailureFallsBackToFreshAuth() async throws {
        let first = MockTransportClient()
        first.enqueueIncoming(.helloAck())
        first.enqueueIncoming(
            .authResult(
                success: true,
                message: "authenticated",
                sessionID: "session-1",
                resumeToken: "resume-1",
                resumeTokenTTLSeconds: 3600
            )
        )

        let second = MockTransportClient()
        second.enqueueIncoming(.helloAck())
        second.enqueueIncoming(
            .resumeResult(
                success: false,
                message: "expired",
                sessionID: nil,
                resumeToken: nil,
                resumeTokenTTLSeconds: nil
            )
        )

        let third = MockTransportClient()
        third.enqueueIncoming(.helloAck())
        third.enqueueIncoming(
            .authResult(
                success: true,
                message: "authenticated",
                sessionID: "session-2",
                resumeToken: "resume-2",
                resumeTokenTTLSeconds: 3600
            )
        )

        let factory = MockTransportFactory(transports: [first, second, third])
        let client = SessionClient(transportClientFactory: { configuration in
            factory.makeTransport(configuration: configuration)
        })

        _ = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token-a" },
            requestVideo: false
        )
        _ = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token-b" },
            requestVideo: false
        )

        XCTAssertEqual(third.sentMessages().map(\.type), [.hello, .authenticate])
        let binding = await client.binding
        XCTAssertEqual(binding?.sessionID, "session-2")
    }

    func testTransportFailurePreservesResumeToken() async throws {
        let first = MockTransportClient()
        first.enqueueIncoming(.helloAck())
        first.enqueueIncoming(
            .authResult(
                success: true,
                message: "authenticated",
                sessionID: "session-1",
                resumeToken: "resume-1",
                resumeTokenTTLSeconds: 3600
            )
        )

        let second = MockTransportClient(connectError: TransportClientError.connectionFailed("boom"))
        let factory = MockTransportFactory(transports: [first, second])
        let client = SessionClient(transportClientFactory: { configuration in
            factory.makeTransport(configuration: configuration)
        })

        _ = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token" },
            requestVideo: false
        )

        await XCTAssertThrowsErrorAsync {
            _ = try await client.connect(
                to: SessionEndpoint(host: "127.0.0.1", port: 4433),
                identityTokenSupplier: { "token" },
                requestVideo: false
            )
        }

        let binding = await client.binding
        XCTAssertEqual(binding?.resumeToken, "resume-1")
    }

    func testVideoDatagramsAreForwardedToHandler() async throws {
        let transport = MockTransportClient()
        transport.enqueueIncoming(.helloAck())
        transport.enqueueIncoming(
            .authResult(
                success: true,
                message: "authenticated",
                sessionID: "session-1",
                resumeToken: "resume-1",
                resumeTokenTTLSeconds: 3600
            )
        )
        transport.yieldDatagram(Data([1, 2, 3]))

        let received = Locked<[Data]>([])
        let client = SessionClient(
            transportClientFactory: { _ in transport },
            onVideoDatagram: { datagram in
                received.withLock { $0.append(datagram) }
            }
        )

        _ = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token" },
            requestVideo: true
        )

        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(received.withLock { $0 }, [Data([1, 2, 3])])
    }

    func testPointerShapeMessagesAreForwardedPostConnect() async throws {
        let transport = MockTransportClient()
        transport.enqueueIncoming(.helloAck())
        transport.enqueueIncoming(
            .authResult(
                success: true,
                message: "authenticated",
                sessionID: "session-1",
                resumeToken: "resume-1",
                resumeTokenTTLSeconds: 3600
            )
        )

        let received = Locked<[ControlMessage]>([])
        let client = SessionClient(
            transportClientFactory: { _ in transport },
            onPointerShapeMessage: { message in
                received.withLock { $0.append(message) }
            }
        )

        _ = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token" },
            requestVideo: true
        )

        transport.enqueueIncoming(
            .pointerShape(
                shapeKind: "color",
                width: 16,
                height: 16,
                hotspotX: 2,
                hotspotY: 3,
                pixelsRGBABase64: "AQIDBA=="
            )
        )

        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(
            received.withLock { $0 },
            [
                .pointerShape(
                    shapeKind: "color",
                    width: 16,
                    height: 16,
                    hotspotX: 2,
                    hotspotY: 3,
                    pixelsRGBABase64: "AQIDBA=="
                )
            ]
        )
    }

    func testInputSendApisUseExpectedTransportShapes() async throws {
        let transport = MockTransportClient()
        transport.enqueueIncoming(.helloAck())
        transport.enqueueIncoming(
            .authResult(
                success: true,
                message: "authenticated",
                sessionID: "session-1",
                resumeToken: "resume-1",
                resumeTokenTTLSeconds: 3600
            )
        )

        let client = SessionClient(transportClientFactory: { _ in transport })
        _ = try await client.connect(
            to: SessionEndpoint(host: "127.0.0.1", port: 4433),
            identityTokenSupplier: { "token" },
            requestVideo: true
        )

        try await client.sendPointerMotion(x: 12, y: 34, sequence: 1)
        try await client.sendPointerButton(button: "left", phase: "down", x: 12, y: 34, sequence: 2)
        try await client.sendWheel(deltaX: 0, deltaY: -120, x: 12, y: 34, sequence: 3)
        try await client.sendKey(keyCode: 4, phase: "up", modifiers: 3)
        try await client.setInputFocus(active: false)

        XCTAssertEqual(
            transport.sentDatagrams(),
            [InputPointerDatagram(sequence: 1, x: 12, y: 34).encode()]
        )
        XCTAssertEqual(
            transport.sentMessages().suffix(4),
            [
                .pointerButton(button: "left", phase: "down", x: 12, y: 34, sequence: 2),
                .pointerWheel(deltaX: 0, deltaY: -120, x: 12, y: 34, sequence: 3),
                .keyboardKey(keyCode: 4, phase: "up", modifiers: 3),
                .inputFocus(active: false),
            ]
        )
    }
}

private final class MockTransportFactory: @unchecked Sendable {
    private let transports: Locked<[MockTransportClient]>

    init(transports: [MockTransportClient]) {
        self.transports = Locked(transports)
    }

    func makeTransport(configuration: TransportConfiguration) -> any TransportClient {
        transports.withLock { transports in
            let transport = transports.removeFirst()
            transport.configurationOverride = configuration
            return transport
        }
    }
}

private final class MockTransportClient: TransportClient, @unchecked Sendable {
    private struct State {
        var incomingMessages: [ControlMessage] = []
        var sentMessages: [ControlMessage] = []
        var sentDatagrams: [Data] = []
        var bufferedDatagrams: [Data] = []
        var datagramContinuation: AsyncThrowingStream<Data, Error>.Continuation?
        var pendingReceive: CheckedContinuation<ControlMessage, Error>?
        var isClosed = false
    }

    let configuredConnectError: Error?
    var configurationOverride = TransportConfiguration()
    var configuration: TransportConfiguration { configurationOverride }

    private let state = Locked(State())

    init(connectError: Error? = nil) {
        self.configuredConnectError = connectError
    }

    func enqueueIncoming(_ message: ControlMessage) {
        let pendingReceive = state.withLock { state -> CheckedContinuation<ControlMessage, Error>? in
            if let pendingReceive = state.pendingReceive {
                state.pendingReceive = nil
                return pendingReceive
            }

            state.incomingMessages.append(message)
            return nil
        }

        pendingReceive?.resume(returning: message)
    }

    func sentMessages() -> [ControlMessage] {
        state.withLock { $0.sentMessages }
    }

    func sentDatagrams() -> [Data] {
        state.withLock { $0.sentDatagrams }
    }

    func yieldDatagram(_ data: Data) {
        state.withLock { state in
            if let continuation = state.datagramContinuation {
                continuation.yield(data)
            } else {
                state.bufferedDatagrams.append(data)
            }
        }
    }

    func connect() async throws {
        if let configuredConnectError {
            throw configuredConnectError
        }
    }

    func armVideoDatagramReceive() -> AsyncThrowingStream<Data, Error> {
        AsyncThrowingStream { continuation in
            let buffered = state.withLock { state -> [Data] in
                state.datagramContinuation = continuation
                let buffered = state.bufferedDatagrams
                state.bufferedDatagrams.removeAll()
                return buffered
            }

            for datagram in buffered {
                continuation.yield(datagram)
            }
        }
    }

    func receive() async throws -> ControlMessage {
        try await withCheckedThrowingContinuation { continuation in
            let immediateMessage = state.withLock { state -> ControlMessage? in
                if !state.incomingMessages.isEmpty {
                    return state.incomingMessages.removeFirst()
                }

                if state.isClosed {
                    continuation.resume(throwing: TransportClientError.connectionClosed)
                    return nil
                }

                state.pendingReceive = continuation
                return nil
            }

            if let immediateMessage {
                continuation.resume(returning: immediateMessage)
            }
        }
    }

    func send(_ message: ControlMessage) async throws {
        state.withLock { $0.sentMessages.append(message) }
    }

    func sendDatagram(_ payload: Data) async throws {
        state.withLock { $0.sentDatagrams.append(payload) }
    }

    func sendHello(clientName: String, capabilities: [String]) async throws {
        try await send(.hello(clientName: clientName, capabilities: capabilities))
    }

    func awaitHelloAck() async throws -> ControlMessage {
        let message = try await receive()
        guard message.type == .helloAck else {
            throw TransportClientError.unexpectedMessage(message.kind)
        }
        return message
    }

    func sendAuthenticate(identityToken: String) async throws {
        try await send(.authenticate(identityToken: identityToken))
    }

    func awaitAuthResult() async throws -> ControlMessage {
        let message = try await receive()
        guard message.type == .authResult else {
            throw TransportClientError.unexpectedMessage(message.kind)
        }
        return message
    }

    func sendResumeSession(resumeToken: String) async throws {
        try await send(.resumeSession(resumeToken: resumeToken))
    }

    func awaitResumeResult() async throws -> ControlMessage {
        let message = try await receive()
        guard message.type == .resumeResult else {
            throw TransportClientError.unexpectedMessage(message.kind)
        }
        return message
    }

    func close(reason: String?) async {
        let (datagramContinuation, pendingReceive) = state.withLock { state -> (AsyncThrowingStream<Data, Error>.Continuation?, CheckedContinuation<ControlMessage, Error>?) in
            let continuation = state.datagramContinuation
            state.datagramContinuation = nil
            let pendingReceive = state.pendingReceive
            state.pendingReceive = nil
            state.isClosed = true
            return (continuation, pendingReceive)
        }
        datagramContinuation?.finish()
        pendingReceive?.resume(throwing: TransportClientError.connectionClosed)
    }
}

private final class Locked<Value>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Value

    init(_ value: Value) {
        self.value = value
    }

    func withLock<Result>(_ body: (inout Value) -> Result) -> Result {
        lock.lock()
        defer { lock.unlock() }
        return body(&value)
    }
}

private func XCTAssertThrowsErrorAsync(
    _ expression: @escaping () async throws -> Void,
    file: StaticString = #filePath,
    line: UInt = #line
) async {
    do {
        try await expression()
        XCTFail("expected error", file: file, line: line)
    } catch {}
}
