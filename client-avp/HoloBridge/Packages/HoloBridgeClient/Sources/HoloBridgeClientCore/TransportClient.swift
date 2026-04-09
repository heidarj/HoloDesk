import Foundation

public enum TransportClientError: Error, LocalizedError, Sendable, Equatable {
    case invalidConfiguration(String)
    case notConnected
    case connectionFailed(String)
    case connectionClosed
    case unexpectedMessage(String)
    case codec(ControlMessageCodecError)
    case authFailed(String)

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
        case .authFailed(let detail):
            return "Authentication failed: \(detail)"
        }
    }
}

public protocol TransportClient: AnyObject, Sendable {
    var configuration: TransportConfiguration { get }

    func connect() async throws
    func armVideoDatagramReceive() -> AsyncThrowingStream<Data, Error>
    func receive() async throws -> ControlMessage
    func send(_ message: ControlMessage) async throws
    func sendDatagram(_ payload: Data) async throws
    func sendHello(
        clientName: String,
        capabilities: [String]
    ) async throws
    func awaitHelloAck() async throws -> ControlMessage
    func sendAuthenticate(identityToken: String) async throws
    func awaitAuthResult() async throws -> ControlMessage
    func sendResumeSession(resumeToken: String) async throws
    func awaitResumeResult() async throws -> ControlMessage
    func close(reason: String?) async
}

public extension TransportClient {
    nonisolated func sendHello(
        clientName: String = "holobridge-avp",
        capabilities: [String]? = nil
    ) async throws {
        let resolvedCapabilities = capabilities ?? [ControlMessage.controlStreamCapability]
        try await send(
            ControlMessage.hello(
                clientName: clientName,
                capabilities: resolvedCapabilities
            )
        )
    }

    func sendAuthenticate(identityToken: String) async throws {
        try await send(ControlMessage.authenticate(identityToken: identityToken))
    }

    func sendResumeSession(resumeToken: String) async throws {
        try await send(ControlMessage.resumeSession(resumeToken: resumeToken))
    }
}
