import CoreGraphics
import CoreVideo
import Foundation
import Observation
import UIKit

@Observable
public final class VideoRenderer {
    public private(set) var isAwaitingFrame = true
    public private(set) var statusMessage = "Waiting for stream"
    public private(set) var frameSizeDescription = "No video yet"
    public private(set) var framesPresented: UInt64 = 0
    public private(set) var lastErrorMessage: String?
    public private(set) var videoFrameWidth: Int = 0
    public private(set) var videoFrameHeight: Int = 0
    public private(set) var pointerVisible = false
    public private(set) var pointerX: Int = 0
    public private(set) var pointerY: Int = 0
    public private(set) var pointerWidth: Int = 0
    public private(set) var pointerHeight: Int = 0
    public private(set) var pointerHotspotX: Int = 0
    public private(set) var pointerHotspotY: Int = 0
    public private(set) var pointerImage: UIImage?

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
        resetPointerOverlay()
    }

    public func updateFormat(width: Int, height: Int) {
        videoFrameWidth = width
        videoFrameHeight = height
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
        videoFrameWidth = CVPixelBufferGetWidth(pixelBuffer)
        videoFrameHeight = CVPixelBufferGetHeight(pixelBuffer)
        frameSizeDescription = "\(videoFrameWidth)x\(videoFrameHeight)"
        framesPresented &+= 1
    }

    public func updatePointerState(_ pointerState: PointerStateDatagram) {
        pointerVisible = pointerState.visible
        pointerX = Int(pointerState.x)
        pointerY = Int(pointerState.y)
    }

    public func updatePointerShape(
        width: Int,
        height: Int,
        hotspotX: Int,
        hotspotY: Int,
        pixelsRGBA: Data
    ) {
        pointerWidth = width
        pointerHeight = height
        pointerHotspotX = hotspotX
        pointerHotspotY = hotspotY
        pointerImage = Self.makePointerImage(
            width: width,
            height: height,
            pixelsRGBA: pixelsRGBA
        )
        if pointerImage == nil {
            recordRecoverableIssue("Pointer shape payload could not be rendered")
        }
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
        videoFrameWidth = 0
        videoFrameHeight = 0
        resetPointerOverlay()
    }

    public func currentPixelBuffer() -> CVPixelBuffer? {
        lock.lock()
        defer { lock.unlock() }
        return latestPixelBuffer
    }

    private func resetPointerOverlay() {
        pointerVisible = false
        pointerX = 0
        pointerY = 0
        pointerWidth = 0
        pointerHeight = 0
        pointerHotspotX = 0
        pointerHotspotY = 0
        pointerImage = nil
    }

    private static func makePointerImage(
        width: Int,
        height: Int,
        pixelsRGBA: Data
    ) -> UIImage? {
        guard width > 0, height > 0, pixelsRGBA.count == width * height * 4 else {
            return nil
        }

        guard
            let provider = CGDataProvider(data: pixelsRGBA as CFData),
            let image = CGImage(
                width: width,
                height: height,
                bitsPerComponent: 8,
                bitsPerPixel: 32,
                bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider,
                decode: nil,
                shouldInterpolate: false,
                intent: .defaultIntent
            )
        else {
            return nil
        }

        return UIImage(cgImage: image)
    }
}
