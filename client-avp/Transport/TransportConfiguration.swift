import Foundation

public struct TransportConfiguration: Sendable, Equatable {
    public enum CloseBehavior: String, Sendable {
        case clientInitiatedGoodbye
        case serverInitiatedGoodbye
    }

    public var host: String
    public var port: UInt16
    public var serverName: String?
    public var alpn: String
    public var allowInsecureCertificateValidation: Bool
    public var pinnedServerCertificateSHA256: String?
    public var closeBehavior: CloseBehavior

    public init(
        host: String = "127.0.0.1",
        port: UInt16 = 4433,
        serverName: String? = "localhost",
        alpn: String = ControlMessage.defaultALPN,
        allowInsecureCertificateValidation: Bool = false,
        pinnedServerCertificateSHA256: String? = nil,
        closeBehavior: CloseBehavior = .clientInitiatedGoodbye
    ) {
        self.host = host
        self.port = port
        self.serverName = serverName
        self.alpn = alpn
        self.allowInsecureCertificateValidation = allowInsecureCertificateValidation
        self.pinnedServerCertificateSHA256 = pinnedServerCertificateSHA256
        self.closeBehavior = closeBehavior
    }

    public var shouldSendGoodbyeAfterAck: Bool {
        closeBehavior == .clientInitiatedGoodbye
    }
}