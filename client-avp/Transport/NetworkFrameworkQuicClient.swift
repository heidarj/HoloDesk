import CryptoKit
import Foundation
import Network
import Security

@available(visionOS 1.0, iOS 15.0, macOS 12.0, *)
public final class NetworkFrameworkQuicClient: TransportClient {
    public let configuration: TransportConfiguration

    private let queue: DispatchQueue
    private var connection: NWConnection?
    private var framer = ControlMessageFramer()

    public init(
        configuration: TransportConfiguration,
        queue: DispatchQueue = DispatchQueue(label: "HoloBridge.Transport.NetworkFrameworkQuicClient")
    ) {
        self.configuration = configuration
        self.queue = queue
    }

    deinit {
        connection?.cancel()
    }

    public func connect() async throws {
        guard connection == nil else {
            return
        }

        guard let port = NWEndpoint.Port(rawValue: configuration.port) else {
            throw TransportClientError.invalidConfiguration("port \(configuration.port) is not a valid NWEndpoint.Port")
        }

        let parameters = try Self.makeParameters(configuration: configuration, queue: queue)
        let connection = NWConnection(
            host: NWEndpoint.Host(configuration.host),
            port: port,
            using: parameters
        )
        self.connection = connection

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            var didResume = false
            connection.stateUpdateHandler = { state in
                guard !didResume else {
                    return
                }

                switch state {
                case .ready:
                    didResume = true
                    continuation.resume()
                case .failed(let error):
                    didResume = true
                    continuation.resume(throwing: TransportClientError.connectionFailed(String(describing: error)))
                case .cancelled:
                    didResume = true
                    continuation.resume(throwing: TransportClientError.connectionClosed)
                default:
                    break
                }
            }

            connection.start(queue: self.queue)
        }
    }

    public func send(_ message: ControlMessage) async throws {
        guard let connection else {
            throw TransportClientError.notConnected
        }

        do {
            let frame = try ControlMessageCodec.encodeFrame(message)
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                connection.send(content: frame, completion: .contentProcessed { error in
                    if let error {
                        continuation.resume(throwing: TransportClientError.connectionFailed(String(describing: error)))
                    } else {
                        continuation.resume()
                    }
                })
            }
        } catch let error as ControlMessageCodecError {
            throw TransportClientError.codec(error)
        }
    }

    public func sendHello(
        clientName: String,
        capabilities: [String]
    ) async throws {
        try await send(ControlMessage.hello(clientName: clientName, capabilities: capabilities))
    }

    public func awaitHelloAck() async throws -> ControlMessage {
        let message = try await receiveMessage()
        guard message.type == .helloAck else {
            throw TransportClientError.unexpectedMessage(message.kind)
        }
        return message
    }

    public func close(reason: String?) async {
        if let reason, connection != nil {
            try? await send(.goodbye(reason: reason))
        }
        connection?.cancel()
        connection = nil
        framer.reset()
    }

    private func receiveMessage() async throws -> ControlMessage {
        while true {
            do {
                if let message = try framer.nextMessage() {
                    return message
                }
            } catch let error as ControlMessageCodecError {
                throw TransportClientError.codec(error)
            }

            let chunk = try await receiveChunk()
            framer.append(chunk)
        }
    }

    private func receiveChunk() async throws -> Data {
        guard let connection else {
            throw TransportClientError.notConnected
        }

        return try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
            connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { content, _, isComplete, error in
                if let error {
                    continuation.resume(throwing: TransportClientError.connectionFailed(String(describing: error)))
                    return
                }

                let data = content ?? Data()
                if isComplete, data.isEmpty {
                    continuation.resume(throwing: TransportClientError.connectionClosed)
                } else {
                    continuation.resume(returning: data)
                }
            }
        }
    }

    private static func makeParameters(
        configuration: TransportConfiguration,
        queue: DispatchQueue
    ) throws -> NWParameters {
        let quicOptions = NWProtocolQUIC.Options()
        sec_protocol_options_add_tls_application_protocol(
            quicOptions.securityProtocolOptions,
            configuration.alpn
        )

        let serverName = configuration.serverName ?? configuration.host
        sec_protocol_options_set_tls_server_name(
            quicOptions.securityProtocolOptions,
            serverName
        )

        if configuration.allowInsecureCertificateValidation || configuration.pinnedServerCertificateSHA256 != nil {
            sec_protocol_options_set_verify_block(
                quicOptions.securityProtocolOptions,
                { _, trust, completion in
                    if configuration.allowInsecureCertificateValidation {
                        completion(true)
                        return
                    }

                    guard let expectedFingerprint = configuration.pinnedServerCertificateSHA256 else {
                        completion(false)
                        return
                    }

                    let trustRef = sec_trust_copy_ref(trust).takeRetainedValue()
                    guard let certificate = SecTrustGetCertificateAtIndex(trustRef, 0) else {
                        completion(false)
                        return
                    }

                    let certificateData = SecCertificateCopyData(certificate) as Data
                    let digest = SHA256.hash(data: certificateData)
                    let actualFingerprint = digest.map { String(format: "%02x", $0) }.joined()
                    completion(normalizeFingerprint(actualFingerprint) == normalizeFingerprint(expectedFingerprint))
                },
                queue
            )
        }

        // TODO: Confirm the exact visionOS runtime behavior against a live Windows host during local Milestone 1 validation.
        let parameters = NWParameters(quic: quicOptions)
        parameters.allowLocalEndpointReuse = true
        return parameters
    }

    private static func normalizeFingerprint(_ value: String) -> String {
        value.replacingOccurrences(of: ":", with: "").trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }
}