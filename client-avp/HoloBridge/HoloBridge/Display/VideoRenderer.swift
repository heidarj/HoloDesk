import CoreVideo
import Foundation
import Observation

@Observable
public final class VideoRenderer {
    public private(set) var isAwaitingFrame = true
    public private(set) var statusMessage = "Waiting for stream"
    public private(set) var frameSizeDescription = "No video yet"
    public private(set) var framesPresented: UInt64 = 0
    public private(set) var lastErrorMessage: String?

    @ObservationIgnored private let lock = NSLock()
    @ObservationIgnored private var latestPixelBuffer: CVPixelBuffer?

    public init() {}

    public func prepareForStream() {
        isAwaitingFrame = true
        statusMessage = "Waiting for first frame"
        lastErrorMessage = nil
        if framesPresented == 0 {
            frameSizeDescription = "No video yet"
        }
    }

    public func updateFormat(width: Int, height: Int) {
        frameSizeDescription = "\(width)x\(height)"
        if isAwaitingFrame {
            statusMessage = "Decoder ready"
        }
    }

    public func present(pixelBuffer: CVPixelBuffer) {
        lock.lock()
        latestPixelBuffer = pixelBuffer
        lock.unlock()

        isAwaitingFrame = false
        statusMessage = "Receiving video"
        lastErrorMessage = nil
        frameSizeDescription = "\(CVPixelBufferGetWidth(pixelBuffer))x\(CVPixelBufferGetHeight(pixelBuffer))"
        framesPresented &+= 1
    }

    public func recordRecoverableIssue(_ message: String) {
        lastErrorMessage = message
        if isAwaitingFrame {
            statusMessage = "Waiting for keyframe"
        }
    }

    public func reset(statusMessage: String = "Waiting for stream") {
        lock.lock()
        latestPixelBuffer = nil
        lock.unlock()

        isAwaitingFrame = true
        self.statusMessage = statusMessage
        frameSizeDescription = "No video yet"
        framesPresented = 0
        lastErrorMessage = nil
    }

    public func currentPixelBuffer() -> CVPixelBuffer? {
        lock.lock()
        defer { lock.unlock() }
        return latestPixelBuffer
    }
}
