import Foundation

public enum TransportClientError: Error, LocalizedError, Sendable, Equatable {
    case invalidConfiguration(String)
    case notConnected
    case connectionFailed(String)
    case connectionClosed
    case unexpectedMessage(String)
    case codec(ControlMessageCodecError)

    public var errorDescription: String? {
        switch self {
        case .invalidConfiguration(let detail):
            return "Invalid transport configuration: \(detail)"
        case .notConnected:
            return "QUIC control stream is not connected"
        case .connectionFailed(let detail):
            return "QUIC connection failed: \(detail)"
        case .connectionClosed:
            return "QUIC connection closed before the expected control message arrived"
        case .unexpectedMessage(let type):
            return "Unexpected control message: \(type)"
        case .codec(let error):
            return error.errorDescription
        }
    }
}

public protocol TransportClient: AnyObject {
    var configuration: TransportConfiguration { get }

    func connect() async throws
    func send(_ message: ControlMessage) async throws
    func sendHello(
        clientName: String,
        capabilities: [String]
    ) async throws
    func awaitHelloAck() async throws -> ControlMessage
    func close(reason: String?) async
}

public extension TransportClient {
    func sendHello(
        clientName: String = "transport-smoke",
        capabilities: [String] = [ControlMessage.controlStreamCapability]
    ) async throws {
        try await send(ControlMessage.hello(clientName: clientName, capabilities: capabilities))
    }
}