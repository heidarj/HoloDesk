import Foundation

enum QuicBridgeEventKind {
    case startingTransport
    case groupReady
    case groupFailed
    case groupCancelled
    case controlStreamExtracted
    case controlStreamReady
    case controlStreamFailed
    case controlStreamCancelled
    case controlPayloadSent
    case controlPayloadReceived
    case datagramReceived
    case closeInitiated
    case closeCompleted
}

struct QuicBridgeEvent {
    let kind: QuicBridgeEventKind
    let detail: String?
}

protocol QuicConnectionBridging: AnyObject {
    var onEvent: ((QuicBridgeEvent) -> Void)? { get set }
    var onControlPayload: ((Data) -> Void)? { get set }
    var onDatagramPayload: ((Data) -> Void)? { get set }
    var onTermination: ((Error?) -> Void)? { get set }

    func start(_ completion: @escaping @Sendable (Error?) -> Void)
    func sendControlPayload(_ payload: Data, completion: @escaping @Sendable (Error?) -> Void)
    func sendDatagramPayload(_ payload: Data, completion: @escaping @Sendable (Error?) -> Void)
    func close(reason: String?)
}
