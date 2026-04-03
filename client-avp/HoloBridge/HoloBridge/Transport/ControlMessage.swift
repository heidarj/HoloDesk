import Foundation

public enum ControlMessageType: String, Codable, Sendable {
    case hello
    case helloAck = "hello_ack"
    case goodbye
    case authenticate
    case authResult = "auth_result"
}

public enum ControlMessageCodecError: Error, LocalizedError, Sendable, Equatable {
    case frameTooShort(Int)
    case frameTooLarge(Int)
    case lengthMismatch(declared: Int, actual: Int)
    case invalidJSON(String)
    case unsupportedProtocolVersion(Int)

    public var errorDescription: String? {
        switch self {
        case .frameTooShort(let actual):
            return "Frame shorter than 4-byte prefix: \(actual) bytes"
        case .frameTooLarge(let actual):
            return "Frame payload too large to encode: \(actual) bytes"
        case .lengthMismatch(let declared, let actual):
            return "Frame length mismatch. Declared \(declared), actual \(actual)"
        case .invalidJSON(let error):
            return "Invalid control message JSON: \(error)"
        case .unsupportedProtocolVersion(let version):
            return "Unsupported protocol version: \(version)"
        }
    }
}

public struct ControlMessage: Codable, Sendable, Equatable {
    public static let protocolVersion = 1
    public static let defaultALPN = "holobridge-m2"
    public static let controlStreamCapability = "control-stream-v1"

    public let type: ControlMessageType
    public let protocolVersion: Int?
    public let clientName: String?
    public let capabilities: [String]?
    public let message: String?
    public let reason: String?
    public let identityToken: String?
    public let success: Bool?
    public let userDisplayName: String?

    enum CodingKeys: String, CodingKey {
        case type
        case protocolVersion = "protocol_version"
        case clientName = "client_name"
        case capabilities
        case message
        case reason
        case identityToken = "identity_token"
        case success
        case userDisplayName = "user_display_name"
    }

    public init(
        type: ControlMessageType,
        protocolVersion: Int? = nil,
        clientName: String? = nil,
        capabilities: [String]? = nil,
        message: String? = nil,
        reason: String? = nil,
        identityToken: String? = nil,
        success: Bool? = nil,
        userDisplayName: String? = nil
    ) {
        self.type = type
        self.protocolVersion = protocolVersion
        self.clientName = clientName
        self.capabilities = capabilities
        self.message = message
        self.reason = reason
        self.identityToken = identityToken
        self.success = success
        self.userDisplayName = userDisplayName
    }

    public static func hello(
        clientName: String = "holobridge-avp",
        capabilities: [String] = [ControlMessage.controlStreamCapability]
    ) -> ControlMessage {
        ControlMessage(
            type: .hello,
            protocolVersion: protocolVersion,
            clientName: clientName,
            capabilities: capabilities
        )
    }

    public static func helloAck(message: String = "ok") -> ControlMessage {
        ControlMessage(type: .helloAck, protocolVersion: protocolVersion, message: message)
    }

    public static func goodbye(reason: String) -> ControlMessage {
        ControlMessage(type: .goodbye, reason: reason)
    }

    public static func authenticate(identityToken: String) -> ControlMessage {
        ControlMessage(type: .authenticate, identityToken: identityToken)
    }

    public static func authResult(
        success: Bool,
        message: String,
        userDisplayName: String? = nil
    ) -> ControlMessage {
        ControlMessage(type: .authResult, message: message, success: success, userDisplayName: userDisplayName)
    }

    public var kind: String {
        type.rawValue
    }
}

public enum ControlMessageCodec {
    public static func encodeFrame(_ message: ControlMessage) throws -> Data {
        let payload = try JSONEncoder().encode(message)
        guard payload.count <= Int(UInt32.max) else {
            throw ControlMessageCodecError.frameTooLarge(payload.count)
        }

        let length = UInt32(payload.count).bigEndian
        let prefix = withUnsafeBytes(of: length) { Data($0) }
        return prefix + payload
    }

    public static func decodeFrame(_ frame: Data) throws -> ControlMessage {
        guard frame.count >= 4 else {
            throw ControlMessageCodecError.frameTooShort(frame.count)
        }

        let declared = decodeLength(frame.prefix(4))
        let payload = frame.dropFirst(4)
        guard payload.count == declared else {
            throw ControlMessageCodecError.lengthMismatch(declared: declared, actual: payload.count)
        }

        do {
            let message = try JSONDecoder().decode(ControlMessage.self, from: Data(payload))
            try validateProtocolVersion(message)
            return message
        } catch let error as ControlMessageCodecError {
            throw error
        } catch {
            throw ControlMessageCodecError.invalidJSON(String(describing: error))
        }
    }

    private static func validateProtocolVersion(_ message: ControlMessage) throws {
        guard let version = message.protocolVersion else {
            return
        }
        guard version == ControlMessage.protocolVersion else {
            throw ControlMessageCodecError.unsupportedProtocolVersion(version)
        }
    }

    private static func decodeLength(_ prefix: Data.SubSequence) -> Int {
        prefix.reduce(0) { partialResult, byte in
            (partialResult << 8) | Int(byte)
        }
    }
}

public struct ControlMessageFramer: Sendable {
    private var buffer = Data()

    public init() {}

    public mutating func append(_ data: Data) {
        buffer.append(data)
    }

    public mutating func nextMessage() throws -> ControlMessage? {
        guard buffer.count >= 4 else {
            return nil
        }

        let declared = buffer.prefix(4).reduce(0) { partialResult, byte in
            (partialResult << 8) | Int(byte)
        }
        guard buffer.count >= 4 + declared else {
            return nil
        }

        let frame = buffer.prefix(4 + declared)
        buffer.removeFirst(4 + declared)
        return try ControlMessageCodec.decodeFrame(Data(frame))
    }

    public mutating func reset() {
        buffer.removeAll(keepingCapacity: false)
    }
}
