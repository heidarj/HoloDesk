import Foundation
import HoloBridgeClientQuicBridge

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

    func start(_ completion: @escaping (Error?) -> Void)
    func sendControlPayload(_ payload: Data, completion: @escaping (Error?) -> Void)
    func sendDatagramPayload(_ payload: Data, completion: @escaping (Error?) -> Void)
    func close(reason: String?)
}

final class ObjectiveCQuicConnectionBridgeAdapter: QuicConnectionBridging {
    var onEvent: ((QuicBridgeEvent) -> Void)?
    var onControlPayload: ((Data) -> Void)?
    var onDatagramPayload: ((Data) -> Void)?
    var onTermination: ((Error?) -> Void)?

    private let bridge: HBQuicConnectionBridge

    init(configuration: TransportConfiguration, queueLabel: String) {
        bridge = HBQuicConnectionBridge(
            host: configuration.host,
            port: configuration.port,
            serverName: configuration.serverName,
            alpn: configuration.alpn,
            allowInsecureCertAuth: configuration.allowInsecureCertificateValidation,
            pinnedCertificateFingerprint: configuration.pinnedServerCertificateSHA256,
            queueLabel: queueLabel
        )

        bridge.eventHandler = { [weak self] eventType, detail in
            self?.onEvent?(QuicBridgeEvent(kind: Self.mapEventKind(eventType), detail: detail))
        }
        bridge.controlPayloadHandler = { [weak self] payload in
            self?.onControlPayload?(payload)
        }
        bridge.datagramHandler = { [weak self] payload in
            self?.onDatagramPayload?(payload)
        }
        bridge.terminationHandler = { [weak self] error in
            self?.onTermination?(error)
        }
    }

    func start(_ completion: @escaping (Error?) -> Void) {
        bridge.start { error in
            completion(error)
        }
    }

    func sendControlPayload(_ payload: Data, completion: @escaping (Error?) -> Void) {
        bridge.sendControlPayload(payload) { error in
            completion(error)
        }
    }

    func sendDatagramPayload(_ payload: Data, completion: @escaping (Error?) -> Void) {
        bridge.sendDatagramPayload(payload) { error in
            completion(error)
        }
    }

    func close(reason: String?) {
        bridge.close(withReason: reason)
    }

    private static func mapEventKind(_ eventType: HBQuicBridgeEventType) -> QuicBridgeEventKind {
        switch eventType {
        case .startingTransport:
            return .startingTransport
        case .groupReady:
            return .groupReady
        case .groupFailed:
            return .groupFailed
        case .groupCancelled:
            return .groupCancelled
        case .controlStreamExtracted:
            return .controlStreamExtracted
        case .controlStreamReady:
            return .controlStreamReady
        case .controlStreamFailed:
            return .controlStreamFailed
        case .controlStreamCancelled:
            return .controlStreamCancelled
        case .controlPayloadSent:
            return .controlPayloadSent
        case .controlPayloadReceived:
            return .controlPayloadReceived
        case .datagramReceived:
            return .datagramReceived
        case .closeInitiated:
            return .closeInitiated
        case .closeCompleted:
            return .closeCompleted
        @unknown default:
            return .closeCompleted
        }
    }
}
