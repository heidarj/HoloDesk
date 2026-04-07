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
    private var datagramsReceived: UInt64 = 0
    private var accessUnitsDecoded: UInt64 = 0
    private var datagramErrors: UInt64 = 0
    private var lastStatsLog = Date.distantPast

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
        datagramsReceived += 1
        do {
            if let accessUnit = try reassembler.push(datagram: datagram) {
                try decoder.decode(accessUnit: accessUnit)
                accessUnitsDecoded += 1
            }
        } catch {
            datagramErrors += 1
            logger.warning("Video datagram dropped: \(error.localizedDescription, privacy: .public)")
            renderer.recordRecoverableIssue(error.localizedDescription)
        }

        let now = Date()
        if now.timeIntervalSince(lastStatsLog) >= 2.0 {
            lastStatsLog = now
            logger.info("video stats: datagrams=\(self.datagramsReceived) decoded=\(self.accessUnitsDecoded) presented=\(self.renderer.framesPresented) errors=\(self.datagramErrors)")
        }
    }

    func reset(statusMessage: String = "Waiting for stream") {
        if datagramsReceived > 0 {
            logger.info("video final: datagrams=\(self.datagramsReceived) decoded=\(self.accessUnitsDecoded) presented=\(self.renderer.framesPresented) errors=\(self.datagramErrors)")
        }
        datagramsReceived = 0
        accessUnitsDecoded = 0
        datagramErrors = 0
        lastStatsLog = .distantPast
        reassembler = H264VideoDatagramReassembler()
        decoder.reset()
        renderer.reset(statusMessage: statusMessage)
    }
}
