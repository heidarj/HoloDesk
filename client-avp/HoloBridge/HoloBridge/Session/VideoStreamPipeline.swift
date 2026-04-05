import Foundation
import os

actor VideoStreamPipeline {
    private let logger = Logger(subsystem: "HoloBridge", category: "VideoPipeline")
    private let renderer: VideoRenderer
    private var reassembler = H264VideoDatagramReassembler()
    private var decoder: H264VideoDecoder

    init(renderer: VideoRenderer) {
        self.renderer = renderer
        self.decoder = H264VideoDecoder(
            onFrameDecoded: { pixelBuffer in
                Task { @MainActor in
                    renderer.present(pixelBuffer: pixelBuffer)
                }
            },
            onFormatDescriptionUpdated: { dimensions in
                Task { @MainActor in
                    renderer.updateFormat(width: Int(dimensions.width), height: Int(dimensions.height))
                }
            },
            onIssue: { message in
                Task { @MainActor in
                    renderer.recordRecoverableIssue(message)
                }
            }
        )
    }

    func prepareForStream() async {
        reassembler = H264VideoDatagramReassembler()
        decoder.reset()
        await MainActor.run {
            renderer.prepareForStream()
        }
    }

    func consume(datagram: Data) async {
        do {
            if let accessUnit = try reassembler.push(datagram: datagram) {
                try decoder.decode(accessUnit: accessUnit)
            }
        } catch {
            logger.warning("Video datagram dropped: \(error.localizedDescription, privacy: .public)")
            await MainActor.run {
                renderer.recordRecoverableIssue(error.localizedDescription)
            }
        }
    }

    func reset(statusMessage: String = "Waiting for stream") async {
        reassembler = H264VideoDatagramReassembler()
        decoder.reset()
        await MainActor.run {
            renderer.reset(statusMessage: statusMessage)
        }
    }
}
