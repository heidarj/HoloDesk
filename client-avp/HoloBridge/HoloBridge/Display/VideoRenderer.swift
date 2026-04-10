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

    public func updatePointerState(x: Int32, y: Int32, visible: Bool) {
        pointerVisible = visible
        pointerX = Int(x)
        pointerY = Int(y)
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

    #if DEBUG
    public func installPreviewTestPattern(
        width: Int = 1920,
        height: Int = 1080
    ) {
        guard let pixelBuffer = Self.makePreviewPixelBuffer(width: width, height: height) else {
            updateFormat(width: width, height: height)
            lastErrorMessage = "Preview frame generation failed"
            return
        }

        present(pixelBuffer: pixelBuffer)
        statusMessage = "Preview feed"
    }
    #endif

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

    #if DEBUG
    private struct PreviewYuvColor {
        let luma: UInt8
        let cb: UInt8
        let cr: UInt8
    }

    private static func makePreviewPixelBuffer(
        width: Int,
        height: Int
    ) -> CVPixelBuffer? {
        let adjustedWidth = max(width - (width % 2), 2)
        let adjustedHeight = max(height - (height % 2), 2)
        let attributes: [CFString: Any] = [
            kCVPixelBufferIOSurfacePropertiesKey: [:],
            kCVPixelBufferMetalCompatibilityKey: true,
        ]

        var pixelBuffer: CVPixelBuffer?
        let status = CVPixelBufferCreate(
            kCFAllocatorDefault,
            adjustedWidth,
            adjustedHeight,
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            attributes as CFDictionary,
            &pixelBuffer
        )

        guard
            status == kCVReturnSuccess,
            let pixelBuffer,
            CVPixelBufferGetPlaneCount(pixelBuffer) == 2
        else {
            return nil
        }

        CVPixelBufferLockBaseAddress(pixelBuffer, [])
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, []) }

        guard
            let lumaBaseAddress = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 0),
            let chromaBaseAddress = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 1)
        else {
            return nil
        }

        let lumaBytesPerRow = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 0)
        let chromaBytesPerRow = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 1)
        let lumaPlane = lumaBaseAddress.assumingMemoryBound(to: UInt8.self)
        let chromaPlane = chromaBaseAddress.assumingMemoryBound(to: UInt8.self)

        let palette = previewPalette()
        let barWidth = max(adjustedWidth / palette.count, 1)
        let footerStart = adjustedHeight * 3 / 4

        for row in 0..<adjustedHeight {
            let lumaRow = lumaPlane.advanced(by: row * lumaBytesPerRow)
            for column in 0..<adjustedWidth {
                let barIndex = min(column / barWidth, palette.count - 1)
                var luma = palette[barIndex].luma

                if row >= footerStart {
                    let checker = ((column / 32) + (row / 32)).isMultiple(of: 2)
                    luma = checker ? 224 : 30
                }

                if column % 96 == 0 || row % 96 == 0 {
                    luma = 235
                }

                if row < 96 {
                    luma = min(luma &+ 12, 235)
                }

                lumaRow[column] = luma
            }
        }

        for row in 0..<(adjustedHeight / 2) {
            let chromaRow = chromaPlane.advanced(by: row * chromaBytesPerRow)
            let sourceRow = row * 2
            for column in 0..<(adjustedWidth / 2) {
                let sourceColumn = column * 2
                let barIndex = min(sourceColumn / barWidth, palette.count - 1)
                var color = palette[barIndex]

                if sourceRow >= footerStart {
                    color = PreviewYuvColor(luma: color.luma, cb: 128, cr: 128)
                }

                let chromaOffset = column * 2
                chromaRow[chromaOffset] = color.cb
                chromaRow[chromaOffset + 1] = color.cr
            }
        }

        return pixelBuffer
    }

    private static func previewPalette() -> [PreviewYuvColor] {
        [
            previewYuvColor(red: 242, green: 244, blue: 248),
            previewYuvColor(red: 245, green: 196, blue: 67),
            previewYuvColor(red: 66, green: 212, blue: 244),
            previewYuvColor(red: 92, green: 220, blue: 126),
            previewYuvColor(red: 243, green: 115, blue: 88),
            previewYuvColor(red: 165, green: 111, blue: 255),
            previewYuvColor(red: 40, green: 42, blue: 54),
            previewYuvColor(red: 18, green: 18, blue: 24),
        ]
    }

    private static func previewYuvColor(
        red: UInt8,
        green: UInt8,
        blue: UInt8
    ) -> PreviewYuvColor {
        let red = Double(red)
        let green = Double(green)
        let blue = Double(blue)
        let luma = clampByte((0.299 * red) + (0.587 * green) + (0.114 * blue))
        let cb = clampByte(128 - (0.168736 * red) - (0.331264 * green) + (0.5 * blue))
        let cr = clampByte(128 + (0.5 * red) - (0.418688 * green) - (0.081312 * blue))
        return PreviewYuvColor(luma: luma, cb: cb, cr: cr)
    }

    private static func clampByte(_ value: Double) -> UInt8 {
        UInt8(max(0, min(255, Int(value.rounded()))))
    }
    #endif
}
