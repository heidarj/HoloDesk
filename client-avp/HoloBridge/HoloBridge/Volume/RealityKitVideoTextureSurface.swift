import CoreGraphics
import CoreImage
import CoreVideo
import Foundation
import Metal
import RealityKit
import UIKit

@MainActor
final class RealityKitVideoTextureSurface {
    private let device: MTLDevice?
    private let ciContext: CIContext?
    private let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()

    private weak var renderer: VideoRenderer?
    private var textureResource: TextureResource?
    private var drawableQueue: TextureResource.DrawableQueue?
    private var textureDimensions = CGSize.zero
    private var lastPresentedFrameCount: UInt64 = 0
    private var displayLink: CADisplayLink?

    init(renderer: VideoRenderer? = nil) {
        let device = MTLCreateSystemDefaultDevice()
        self.device = device
        self.renderer = renderer
        if let device {
            self.ciContext = CIContext(
                mtlDevice: device,
                options: [.cacheIntermediates: false]
            )
        } else {
            self.ciContext = nil
        }
    }

    deinit {
        displayLink?.invalidate()
    }

    func bind(renderer: VideoRenderer) {
        if self.renderer !== renderer {
            lastPresentedFrameCount = 0
        }
        self.renderer = renderer
    }

    func material(
        preferredTextureSize: CGSize
    ) -> UnlitMaterial? {
        ensureTextureResource(size: preferredTextureSize)
        guard let textureResource else {
            return nil
        }

        return UnlitMaterial(texture: textureResource)
    }

    func start() {
        guard displayLink == nil else {
            return
        }

        let displayLink = CADisplayLink(
            target: self,
            selector: #selector(handleDisplayLink(_:))
        )
        displayLink.add(to: .main, forMode: .common)
        self.displayLink = displayLink
    }

    func stop() {
        displayLink?.invalidate()
        displayLink = nil
        lastPresentedFrameCount = 0
    }

    @objc
    private func handleDisplayLink(
        _ displayLink: CADisplayLink
    ) {
        renderLatestFrameIfNeeded()
    }

    private func renderLatestFrameIfNeeded() {
        guard
            let renderer,
            let ciContext,
            let pixelBuffer = renderer.currentPixelBuffer()
        else {
            return
        }

        let frameCount = renderer.framesPresented
        guard frameCount != lastPresentedFrameCount else {
            return
        }

        let pixelBufferSize = CGSize(
            width: CVPixelBufferGetWidth(pixelBuffer),
            height: CVPixelBufferGetHeight(pixelBuffer)
        )
        ensureTextureResource(size: pixelBufferSize)

        guard
            let drawableQueue,
            let drawable = try? drawableQueue.nextDrawable()
        else {
            return
        }

        let image = CIImage(cvPixelBuffer: pixelBuffer)
        ciContext.render(
            image,
            to: drawable.texture,
            commandBuffer: nil,
            bounds: CGRect(origin: .zero, size: pixelBufferSize),
            colorSpace: colorSpace
        )
        drawable.presentOnSceneUpdate()
        lastPresentedFrameCount = frameCount
    }

    private func ensureTextureResource(
        size: CGSize
    ) {
        guard device != nil else {
            return
        }

        let validatedSize = Self.validatedTextureSize(size)
        guard validatedSize != textureDimensions || textureResource == nil || drawableQueue == nil else {
            return
        }

        guard
            let queue = try? TextureResource.DrawableQueue(
                .init(
                    pixelFormat: .bgra8Unorm,
                    width: Int(validatedSize.width),
                    height: Int(validatedSize.height),
                    usage: [.shaderRead, .shaderWrite, .renderTarget],
                    mipmapsMode: .none
                )
            ),
            let blackImage = Self.makeBlackImage(
                width: Int(validatedSize.width),
                height: Int(validatedSize.height)
            ),
            let textureResource = try? TextureResource(
                image: blackImage,
                withName: "CurvedDisplayVideoTexture",
                options: .init(semantic: .color, mipmapsMode: .none)
            )
        else {
            return
        }

        textureResource.replace(withDrawables: queue)
        textureDimensions = validatedSize
        drawableQueue = queue
        self.textureResource = textureResource
        lastPresentedFrameCount = 0
    }

    private static func validatedTextureSize(
        _ size: CGSize
    ) -> CGSize {
        let width = max(Int(size.width.rounded(.up)), 1)
        let height = max(Int(size.height.rounded(.up)), 1)
        guard width > 1, height > 1 else {
            return CGSize(width: 1280, height: 720)
        }
        return CGSize(width: width, height: height)
    }

    private static func makeBlackImage(
        width: Int,
        height: Int
    ) -> CGImage? {
        guard
            width > 0,
            height > 0,
            let provider = CGDataProvider(data: Data(count: width * height * 4) as CFData)
        else {
            return nil
        }

        return CGImage(
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
    }
}
