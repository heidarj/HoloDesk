import CoreMedia
import Foundation
import HoloBridgeClientCore
import os

@MainActor
final class VideoStreamPipeline {
    private let logger = Logger(subsystem: "HoloBridge", category: "VideoPipeline")
    private let renderer: VideoRenderer
    private var reassembler = H264VideoDatagramReassembler()
    private var decoder: H264VideoDecoder

    init(renderer: VideoRenderer) {
        self.renderer = renderer
        self.decoder = H264VideoDecoder(
            onFrameDecoded: { pixelBuffer in
                renderer.present(pixelBuffer: pixelBuffer)
            },
            onFormatDescriptionUpdated: { dimensions in
                renderer.updateFormat(
                    width: Int(dimensions.width),
                    height: Int(dimensions.height)
                )
            },
            onIssue: { message in
                renderer.recordRecoverableIssue(message)
            }
        )
    }

    func prepareForStream() {
        reassembler = H264VideoDatagramReassembler()
        decoder.reset()
        renderer.prepareForStream()
    }

    func consume(datagram: Data) {
        do {
            if let accessUnit = try reassembler.push(datagram: datagram) {
                try decoder.decode(accessUnit: accessUnit)
            }
        } catch {
            logger.warning("Video datagram dropped: \(error.localizedDescription, privacy: .public)")
            renderer.recordRecoverableIssue(error.localizedDescription)
        }
    }

    func reset(statusMessage: String = "Waiting for stream") {
        reassembler = H264VideoDatagramReassembler()
        decoder.reset()
        renderer.reset(statusMessage: statusMessage)
    }
}
