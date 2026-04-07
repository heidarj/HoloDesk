import Foundation

public enum ControlMessageType: String, Codable, Sendable {
    case hello
    case helloAck = "hello_ack"
    case goodbye
    case authenticate
    case resumeSession = "resume_session"
    case authResult = "auth_result"
    case resumeResult = "resume_result"
    case pointerShape = "pointer_shape"
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
    public static let videoDatagramCapability = "video-datagram-h264-v1"
    public static let pointerDatagramCapability = "pointer-datagram-v1"
    public static let pointerStreamCapability = "pointer-stream-v1"

    public let type: ControlMessageType
    public let protocolVersion: Int?
    public let clientName: String?
    public let capabilities: [String]?
    public let message: String?
    public let reason: String?
    public let identityToken: String?
    public let resumeToken: String?
    public let success: Bool?
    public let userDisplayName: String?
    public let sessionID: String?
    public let resumeTokenTTLSeconds: UInt64?
    public let shapeKind: String?
    public let width: UInt32?
    public let height: UInt32?
    public let hotspotX: Int32?
    public let hotspotY: Int32?
    public let pixelsRGBABase64: String?

    enum CodingKeys: String, CodingKey {
        case type
        case protocolVersion = "protocol_version"
        case clientName = "client_name"
        case capabilities
        case message
        case reason
        case identityToken = "identity_token"
        case resumeToken = "resume_token"
        case success
        case userDisplayName = "user_display_name"
        case sessionID = "session_id"
        case resumeTokenTTLSeconds = "resume_token_ttl_secs"
        case shapeKind = "shape_kind"
        case width
        case height
        case hotspotX = "hotspot_x"
        case hotspotY = "hotspot_y"
        case pixelsRGBABase64 = "pixels_rgba_base64"
    }

    public init(
        type: ControlMessageType,
        protocolVersion: Int? = nil,
        clientName: String? = nil,
        capabilities: [String]? = nil,
        message: String? = nil,
        reason: String? = nil,
        identityToken: String? = nil,
        resumeToken: String? = nil,
        success: Bool? = nil,
        userDisplayName: String? = nil,
        sessionID: String? = nil,
        resumeTokenTTLSeconds: UInt64? = nil,
        shapeKind: String? = nil,
        width: UInt32? = nil,
        height: UInt32? = nil,
        hotspotX: Int32? = nil,
        hotspotY: Int32? = nil,
        pixelsRGBABase64: String? = nil
    ) {
        self.type = type
        self.protocolVersion = protocolVersion
        self.clientName = clientName
        self.capabilities = capabilities
        self.message = message
        self.reason = reason
        self.identityToken = identityToken
        self.resumeToken = resumeToken
        self.success = success
        self.userDisplayName = userDisplayName
        self.sessionID = sessionID
        self.resumeTokenTTLSeconds = resumeTokenTTLSeconds
        self.shapeKind = shapeKind
        self.width = width
        self.height = height
        self.hotspotX = hotspotX
        self.hotspotY = hotspotY
        self.pixelsRGBABase64 = pixelsRGBABase64
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

    public static func resumeSession(resumeToken: String) -> ControlMessage {
        ControlMessage(type: .resumeSession, resumeToken: resumeToken)
    }

    public static func authResult(
        success: Bool,
        message: String,
        userDisplayName: String? = nil,
        sessionID: String? = nil,
        resumeToken: String? = nil,
        resumeTokenTTLSeconds: UInt64? = nil
    ) -> ControlMessage {
        ControlMessage(
            type: .authResult,
            message: message,
            resumeToken: resumeToken,
            success: success,
            userDisplayName: userDisplayName,
            sessionID: sessionID,
            resumeTokenTTLSeconds: resumeTokenTTLSeconds
        )
    }

    public static func resumeResult(
        success: Bool,
        message: String,
        userDisplayName: String? = nil,
        sessionID: String? = nil,
        resumeToken: String? = nil,
        resumeTokenTTLSeconds: UInt64? = nil
    ) -> ControlMessage {
        ControlMessage(
            type: .resumeResult,
            message: message,
            resumeToken: resumeToken,
            success: success,
            userDisplayName: userDisplayName,
            sessionID: sessionID,
            resumeTokenTTLSeconds: resumeTokenTTLSeconds
        )
    }

    public static func pointerShape(
        shapeKind: String,
        width: UInt32,
        height: UInt32,
        hotspotX: Int32,
        hotspotY: Int32,
        pixelsRGBABase64: String
    ) -> ControlMessage {
        ControlMessage(
            type: .pointerShape,
            shapeKind: shapeKind,
            width: width,
            height: height,
            hotspotX: hotspotX,
            hotspotY: hotspotY,
            pixelsRGBABase64: pixelsRGBABase64
        )
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
