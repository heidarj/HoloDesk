import Foundation
@testable import HoloBridgeClientCore
import XCTest

final class H264VideoDatagramTests: XCTestCase {
    func testSingleDatagramAccessUnitRoundTrip() throws {
        var reassembler = H264VideoDatagramReassembler()
        let payload = Data([1, 2, 3, 4])

        let datagram = makeDatagram(
            accessUnitID: 7,
            fragmentIndex: 0,
            fragmentCount: 1,
            pts100ns: 16_666,
            duration100ns: 16_666,
            isKeyframe: true,
            payload: payload
        )

        let accessUnit = try reassembler.push(datagram: datagram)
        XCTAssertEqual(
            accessUnit,
            H264VideoAccessUnit(
                accessUnitID: 7,
                data: payload,
                pts100ns: 16_666,
                duration100ns: 16_666,
                isKeyframe: true
            )
        )
    }

    func testMultiDatagramOutOfOrderReassembly() throws {
        var reassembler = H264VideoDatagramReassembler()
        let first = makeDatagram(
            accessUnitID: 11,
            fragmentIndex: 1,
            fragmentCount: 2,
            pts100ns: 1,
            duration100ns: 2,
            isKeyframe: false,
            payload: Data([3, 4])
        )
        let second = makeDatagram(
            accessUnitID: 11,
            fragmentIndex: 0,
            fragmentCount: 2,
            pts100ns: 1,
            duration100ns: 2,
            isKeyframe: false,
            payload: Data([1, 2])
        )

        XCTAssertNil(try reassembler.push(datagram: first))
        let accessUnit = try reassembler.push(datagram: second)
        XCTAssertEqual(accessUnit?.data, Data([1, 2, 3, 4]))
    }

    func testIncompleteAccessUnitExpires() throws {
        var reassembler = H264VideoDatagramReassembler(
            config: .init(incompleteTimeout: 0.01, maxInFlightAccessUnits: 4)
        )
        let now = Date()
        let datagram = makeDatagram(
            accessUnitID: 13,
            fragmentIndex: 0,
            fragmentCount: 2,
            pts100ns: 1,
            duration100ns: 2,
            isKeyframe: false,
            payload: Data([1, 2])
        )

        XCTAssertNil(try reassembler.push(datagram: datagram, now: now))
        reassembler.pruneExpired(now: now.addingTimeInterval(0.1))
        XCTAssertEqual(reassembler.stats.droppedIncompleteAccessUnits, 1)
    }

    func testMetadataMismatchIsRejected() throws {
        var reassembler = H264VideoDatagramReassembler()
        let first = makeDatagram(
            accessUnitID: 17,
            fragmentIndex: 0,
            fragmentCount: 2,
            pts100ns: 1,
            duration100ns: 2,
            isKeyframe: false,
            payload: Data([1, 2])
        )
        let second = makeDatagram(
            accessUnitID: 17,
            fragmentIndex: 1,
            fragmentCount: 2,
            pts100ns: 9,
            duration100ns: 2,
            isKeyframe: false,
            payload: Data([3, 4])
        )

        XCTAssertNil(try reassembler.push(datagram: first))
        XCTAssertThrowsError(try reassembler.push(datagram: second)) { error in
            XCTAssertEqual(
                error as? H264VideoDatagramError,
                .inconsistentFragmentMetadata(17)
            )
        }
    }

    func testPointerStateDatagramDecode() throws {
        var datagram = Data(repeating: 0, count: 24)
        datagram[0] = 1
        datagram[1] = 0x01
        datagram[2] = 1
        datagram.replaceSubrange(4..<12, with: withUnsafeBytes(of: UInt64(99).bigEndian, Array.init))
        datagram.replaceSubrange(12..<16, with: withUnsafeBytes(of: UInt32(bitPattern: Int32(-17)).bigEndian, Array.init))
        datagram.replaceSubrange(16..<20, with: withUnsafeBytes(of: UInt32(bitPattern: Int32(42)).bigEndian, Array.init))

        let parsed = try MediaDatagramParser.decode(datagram)
        XCTAssertEqual(
            parsed,
            .pointerState(
                PointerStateDatagram(
                    sequence: 99,
                    x: -17,
                    y: 42,
                    visible: true
                )
            )
        )
    }

    func testInputPointerDatagramRoundTrip() throws {
        let datagram = InputPointerDatagram(sequence: 42, x: -12, y: 88).encode()
        let decoded = try InputPointerDatagram.decode(datagram)
        XCTAssertEqual(decoded, InputPointerDatagram(sequence: 42, x: -12, y: 88))
    }

    private func makeDatagram(
        accessUnitID: UInt64,
        fragmentIndex: UInt16,
        fragmentCount: UInt16,
        pts100ns: Int64,
        duration100ns: Int64,
        isKeyframe: Bool,
        payload: Data
    ) -> Data {
        var header = Data(repeating: 0, count: 32)
        header[0] = 1
        header[1] = isKeyframe ? 0x01 : 0x00
        header.replaceSubrange(4..<12, with: withUnsafeBytes(of: accessUnitID.bigEndian, Array.init))
        header.replaceSubrange(12..<14, with: withUnsafeBytes(of: fragmentIndex.bigEndian, Array.init))
        header.replaceSubrange(14..<16, with: withUnsafeBytes(of: fragmentCount.bigEndian, Array.init))
        header.replaceSubrange(16..<24, with: withUnsafeBytes(of: UInt64(bitPattern: pts100ns).bigEndian, Array.init))
        header.replaceSubrange(24..<32, with: withUnsafeBytes(of: UInt64(bitPattern: duration100ns).bigEndian, Array.init))
        return header + payload
    }
}
