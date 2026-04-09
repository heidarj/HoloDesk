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

    func testPointerShapeRoundTrip() throws {
        let original = ControlMessage.pointerShape(
            shapeKind: "color",
            width: 24,
            height: 24,
            hotspotX: 3,
            hotspotY: 5,
            pixelsRGBABase64: "AQIDBA=="
        )

        let frame = try ControlMessageCodec.encodeFrame(original)
        let decoded = try ControlMessageCodec.decodeFrame(frame)
        XCTAssertEqual(decoded, original)
    }

    func testInputControlMessagesRoundTrip() throws {
        let messages: [ControlMessage] = [
            .pointerButton(button: "left", phase: "down", x: 101, y: 202, sequence: 9),
            .pointerWheel(deltaX: 0, deltaY: -120, x: 102, y: 203, sequence: 10),
            .keyboardKey(keyCode: 4, phase: "up", modifiers: 3),
            .inputFocus(active: false),
        ]

        for original in messages {
            let frame = try ControlMessageCodec.encodeFrame(original)
            let decoded = try ControlMessageCodec.decodeFrame(frame)
            XCTAssertEqual(decoded, original)
        }
    }
}
