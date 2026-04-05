import Foundation

public struct H264VideoAccessUnit: Sendable, Equatable {
    public let accessUnitID: UInt64
    public let data: Data
    public let pts100ns: Int64
    public let duration100ns: Int64
    public let isKeyframe: Bool
}

public enum H264VideoDatagramError: Error, LocalizedError, Sendable, Equatable {
    case headerTooShort(Int)
    case unsupportedVersion(UInt8)
    case invalidFragmentCount(UInt16)
    case invalidFragmentIndex(index: UInt16, count: UInt16)
    case emptyPayload(UInt64)
    case inconsistentFragmentMetadata(UInt64)

    public var errorDescription: String? {
        switch self {
        case .headerTooShort(let actual):
            return "Media datagram shorter than header: \(actual) bytes"
        case .unsupportedVersion(let actual):
            return "Unsupported media datagram version: \(actual)"
        case .invalidFragmentCount(let actual):
            return "Invalid media datagram fragment count: \(actual)"
        case .invalidFragmentIndex(let index, let count):
            return "Fragment index \(index) is outside fragment count \(count)"
        case .emptyPayload(let accessUnitID):
            return "Received an empty media datagram payload for access unit \(accessUnitID)"
        case .inconsistentFragmentMetadata(let accessUnitID):
            return "Fragment metadata changed within access unit \(accessUnitID)"
        }
    }
}

public struct H264VideoDatagramReassembler: Sendable {
    public struct Config: Sendable, Equatable {
        public var incompleteTimeout: TimeInterval
        public var maxInFlightAccessUnits: Int

        public init(
            incompleteTimeout: TimeInterval = 0.5,
            maxInFlightAccessUnits: Int = 32
        ) {
            self.incompleteTimeout = incompleteTimeout
            self.maxInFlightAccessUnits = maxInFlightAccessUnits
        }
    }

    public struct Stats: Sendable, Equatable {
        public var droppedIncompleteAccessUnits: UInt64 = 0

        public init() {}
    }

    private struct Header: Sendable, Equatable {
        static let version: UInt8 = 1
        static let encodedLength = 32
        static let keyframeFlag: UInt8 = 0x01

        let accessUnitID: UInt64
        let fragmentIndex: UInt16
        let fragmentCount: UInt16
        let pts100ns: Int64
        let duration100ns: Int64
        let isKeyframe: Bool
    }

    private struct IncompleteAccessUnit {
        let header: Header
        let firstSeenAt: Date
        var fragments: [Data?]
        var receivedFragments: Int
    }

    public private(set) var stats = Stats()

    private let config: Config
    private var incompleteAccessUnits: [UInt64: IncompleteAccessUnit] = [:]

    public init(config: Config = Config()) {
        self.config = config
    }

    public mutating func reset() {
        incompleteAccessUnits.removeAll(keepingCapacity: false)
        stats = Stats()
    }

    public mutating func push(
        datagram: Data,
        now: Date = Date()
    ) throws -> H264VideoAccessUnit? {
        pruneExpired(now: now)

        let (header, payload) = try Self.decodeHeader(from: datagram)
        guard !payload.isEmpty else {
            throw H264VideoDatagramError.emptyPayload(header.accessUnitID)
        }

        if header.fragmentCount == 1 {
            return H264VideoAccessUnit(
                accessUnitID: header.accessUnitID,
                data: payload,
                pts100ns: header.pts100ns,
                duration100ns: header.duration100ns,
                isKeyframe: header.isKeyframe
            )
        }

        while incompleteAccessUnits.count >= config.maxInFlightAccessUnits {
            guard
                let oldest = incompleteAccessUnits.min(by: { $0.value.firstSeenAt < $1.value.firstSeenAt })
            else {
                break
            }
            incompleteAccessUnits.removeValue(forKey: oldest.key)
            stats.droppedIncompleteAccessUnits &+= 1
        }

        var entry = incompleteAccessUnits[header.accessUnitID] ?? IncompleteAccessUnit(
            header: header,
            firstSeenAt: now,
            fragments: Array(repeating: nil, count: Int(header.fragmentCount)),
            receivedFragments: 0
        )

        guard entry.header == header else {
            throw H264VideoDatagramError.inconsistentFragmentMetadata(header.accessUnitID)
        }

        let fragmentIndex = Int(header.fragmentIndex)
        if entry.fragments[fragmentIndex] == nil {
            entry.fragments[fragmentIndex] = payload
            entry.receivedFragments += 1
        }

        if entry.receivedFragments != entry.fragments.count {
            incompleteAccessUnits[header.accessUnitID] = entry
            return nil
        }

        incompleteAccessUnits.removeValue(forKey: header.accessUnitID)

        let assembled = entry.fragments.compactMap { $0 }.reduce(into: Data()) { partial, fragment in
            partial.append(fragment)
        }
        return H264VideoAccessUnit(
            accessUnitID: header.accessUnitID,
            data: assembled,
            pts100ns: header.pts100ns,
            duration100ns: header.duration100ns,
            isKeyframe: header.isKeyframe
        )
    }

    public mutating func pruneExpired(now: Date = Date()) {
        let expiration = config.incompleteTimeout
        var retained: [UInt64: IncompleteAccessUnit] = [:]
        retained.reserveCapacity(incompleteAccessUnits.count)

        for (accessUnitID, accessUnit) in incompleteAccessUnits {
            if now.timeIntervalSince(accessUnit.firstSeenAt) < expiration {
                retained[accessUnitID] = accessUnit
            } else {
                stats.droppedIncompleteAccessUnits &+= 1
            }
        }
        incompleteAccessUnits = retained
    }

    private static func decodeHeader(from datagram: Data) throws -> (Header, Data) {
        guard datagram.count >= Header.encodedLength else {
            throw H264VideoDatagramError.headerTooShort(datagram.count)
        }

        let version = datagram[0]
        guard version == Header.version else {
            throw H264VideoDatagramError.unsupportedVersion(version)
        }

        let fragmentIndex = datagram.readUInt16BigEndian(at: 12)
        let fragmentCount = datagram.readUInt16BigEndian(at: 14)
        guard fragmentCount > 0 else {
            throw H264VideoDatagramError.invalidFragmentCount(fragmentCount)
        }
        guard fragmentIndex < fragmentCount else {
            throw H264VideoDatagramError.invalidFragmentIndex(index: fragmentIndex, count: fragmentCount)
        }

        let header = Header(
            accessUnitID: datagram.readUInt64BigEndian(at: 4),
            fragmentIndex: fragmentIndex,
            fragmentCount: fragmentCount,
            pts100ns: datagram.readInt64BigEndian(at: 16),
            duration100ns: datagram.readInt64BigEndian(at: 24),
            isKeyframe: (datagram[1] & Header.keyframeFlag) != 0
        )
        return (header, datagram.subdata(in: Header.encodedLength..<datagram.count))
    }
}

private extension Data {
    func readUInt16BigEndian(at offset: Int) -> UInt16 {
        let range = offset..<(offset + 2)
        return self[range].reduce(into: UInt16(0)) { partial, byte in
            partial = (partial << 8) | UInt16(byte)
        }
    }

    func readUInt64BigEndian(at offset: Int) -> UInt64 {
        let range = offset..<(offset + 8)
        return self[range].reduce(into: UInt64(0)) { partial, byte in
            partial = (partial << 8) | UInt64(byte)
        }
    }

    func readInt64BigEndian(at offset: Int) -> Int64 {
        Int64(bitPattern: readUInt64BigEndian(at: offset))
    }
}
