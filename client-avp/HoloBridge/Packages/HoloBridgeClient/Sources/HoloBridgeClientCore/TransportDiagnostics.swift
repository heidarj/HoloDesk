import Foundation

public struct TransportDiagnosticEvent: Sendable, Equatable {
    public enum Kind: String, Sendable {
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

    public let kind: Kind
    public let detail: String?

    public init(kind: Kind, detail: String? = nil) {
        self.kind = kind
        self.detail = detail
    }
}

public typealias TransportDiagnosticHandler = @Sendable (TransportDiagnosticEvent) -> Void
