import Foundation
@testable import HoloBridgeClientCore
import XCTest

final class ControlMessageCodecTests: XCTestCase {
    func testHelloRoundTrip() throws {
        let original = ControlMessage.hello(
            clientName: "smoke",
            capabilities: [
                ControlMessage.controlStreamCapability,
                ControlMessage.videoDatagramCapability,
            ]
        )

        let frame = try ControlMessageCodec.encodeFrame(original)
        let decoded = try ControlMessageCodec.decodeFrame(frame)
        XCTAssertEqual(decoded, original)
    }

    func testUnsupportedProtocolVersionIsRejected() throws {
        let original = ControlMessage(
            type: .hello,
            protocolVersion: 999,
            clientName: "smoke",
            capabilities: [ControlMessage.controlStreamCapability]
        )

        let frame = try ControlMessageCodec.encodeFrame(original)
        XCTAssertThrowsError(try ControlMessageCodec.decodeFrame(frame)) { error in
            XCTAssertEqual(error as? ControlMessageCodecError, .unsupportedProtocolVersion(999))
        }
    }
}
