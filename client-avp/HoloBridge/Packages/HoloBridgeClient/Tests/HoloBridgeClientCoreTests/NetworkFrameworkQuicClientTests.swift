import Foundation
@testable import HoloBridgeClientCore
import XCTest

final class NetworkFrameworkQuicClientTests: XCTestCase {
    func testConnectAndAwaitHelloAckThroughBridge() async throws {
        let bridge = MockQuicBridge()
        let client = NetworkFrameworkQuicClient(
            configuration: TransportConfiguration(),
            diagnosticHandler: nil,
            bridgeFactory: { _, _ in bridge }
        )

        let connectTask = Task {
            try await client.connect()
        }

        bridge.emitEvent(.startingTransport)
        bridge.emitEvent(.groupReady)
        bridge.emitEvent(.controlStreamExtracted)
        bridge.emitEvent(.controlStreamReady)
        bridge.completeStart(with: nil)

        try await connectTask.value

        let helloAckFrame = try ControlMessageCodec.encodeFrame(.helloAck())
        bridge.emitControlPayload(helloAckFrame)

        let message = try await client.awaitHelloAck()
        XCTAssertEqual(message.type, .helloAck)
    }

    func testDiagnosticsDatagramsAndTerminationAreForwarded() async throws {
        let bridge = MockQuicBridge()
        let diagnostics = Locked<[TransportDiagnosticEvent]>([])
        let client = NetworkFrameworkQuicClient(
            configuration: TransportConfiguration(),
            diagnosticHandler: { event in
                diagnostics.withLock { $0.append(event) }
            },
            bridgeFactory: { _, _ in bridge }
        )

        let connectTask = Task {
            try await client.connect()
        }

        bridge.emitEvent(.startingTransport)
        bridge.emitEvent(.groupReady)
        bridge.emitEvent(.controlStreamExtracted)
        bridge.emitEvent(.controlStreamReady)
        bridge.completeStart(with: nil)
        try await connectTask.value

        let datagramStream = client.armVideoDatagramReceive()
        var iterator = datagramStream.makeAsyncIterator()

        bridge.emitEvent(.datagramReceived, detail: "4")
        bridge.emitDatagramPayload(Data([1, 2, 3, 4]))

        let datagram = try await iterator.next()
        XCTAssertEqual(datagram, Data([1, 2, 3, 4]))

        bridge.emitEvent(.closeCompleted)
        bridge.finish(with: nil)
        let end = try await iterator.next()
        XCTAssertNil(end)

        let kinds = diagnostics.withLock { $0.map(\.kind) }
        XCTAssertTrue(kinds.contains(.startingTransport))
        XCTAssertTrue(kinds.contains(.controlStreamReady))
        XCTAssertTrue(kinds.contains(.datagramReceived))
        XCTAssertTrue(kinds.contains(.closeCompleted))
    }
}

private final class MockQuicBridge: QuicConnectionBridging, @unchecked Sendable {
    private let state = Locked(MockState())

    var onEvent: ((QuicBridgeEvent) -> Void)?
    var onControlPayload: ((Data) -> Void)?
    var onDatagramPayload: ((Data) -> Void)?
    var onTermination: ((Error?) -> Void)?

    func start(_ completion: @escaping (Error?) -> Void) {
        let pendingStartResult = state.withLock { state -> PendingStartResult? in
            if let pendingStartResult = state.pendingStartResult {
                state.pendingStartResult = nil
                return pendingStartResult
            }

            state.startCompletion = completion
            return nil
        }

        guard let pendingStartResult else {
            return
        }

        switch pendingStartResult {
        case .success:
            completion(nil)
        case .failure(let error):
            completion(error)
        }
    }

    func sendControlPayload(_ payload: Data, completion: @escaping (Error?) -> Void) {
        state.withLock { $0.sentControlPayloads.append(payload) }
        completion(nil)
    }

    func sendDatagramPayload(_ payload: Data, completion: @escaping (Error?) -> Void) {
        state.withLock { $0.sentDatagramPayloads.append(payload) }
        completion(nil)
    }

    func close(reason: String?) {
        state.withLock { $0.closeReasons.append(reason) }
    }

    func completeStart(with error: Error?) {
        let completion = state.withLock { state -> ((Error?) -> Void)? in
            let completion = state.startCompletion
            if completion == nil {
                if let error {
                    state.pendingStartResult = .failure(error)
                } else {
                    state.pendingStartResult = .success
                }
            } else {
                state.startCompletion = nil
            }
            return completion
        }
        completion?(error)
    }

    func emitEvent(_ kind: QuicBridgeEventKind, detail: String? = nil) {
        onEvent?(QuicBridgeEvent(kind: kind, detail: detail))
    }

    func emitControlPayload(_ payload: Data) {
        onControlPayload?(payload)
    }

    func emitDatagramPayload(_ payload: Data) {
        onDatagramPayload?(payload)
    }

    func finish(with error: Error?) {
        onTermination?(error)
    }
}

private enum PendingStartResult {
    case success
    case failure(Error)
}

private struct MockState {
    var startCompletion: ((Error?) -> Void)?
    var pendingStartResult: PendingStartResult?
    var sentControlPayloads: [Data] = []
    var sentDatagramPayloads: [Data] = []
    var closeReasons: [String?] = []
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
