import CoreVideo
import Metal
import MetalKit
import SwiftUI
import UIKit

struct VideoDisplayView: UIViewRepresentable {
    let renderer: VideoRenderer

    func makeCoordinator() -> Coordinator {
        Coordinator(renderer: renderer)
    }

    func makeUIView(context: Context) -> MTKView {
        let view = MTKView(frame: .zero, device: context.coordinator.device)
        view.delegate = context.coordinator
        view.preferredFramesPerSecond = 60
        view.enableSetNeedsDisplay = false
        view.isPaused = false
        view.framebufferOnly = false
        view.colorPixelFormat = .bgra8Unorm
        view.clearColor = MTLClearColor(red: 0.03, green: 0.03, blue: 0.04, alpha: 1.0)
        view.layer.cornerRadius = 20
        view.clipsToBounds = true
        return view
    }

    func updateUIView(_ uiView: MTKView, context: Context) {
        uiView.device = context.coordinator.device
    }

    final class Coordinator: NSObject, MTKViewDelegate {
        let renderer: VideoRenderer
        let device: MTLDevice?

        private let commandQueue: MTLCommandQueue?
        private var textureCache: CVMetalTextureCache?
        private let pipelineState: MTLRenderPipelineState?

        init(renderer: VideoRenderer) {
            self.renderer = renderer
            self.device = MTLCreateSystemDefaultDevice()
            self.commandQueue = device?.makeCommandQueue()

            if let device {
                var textureCache: CVMetalTextureCache?
                CVMetalTextureCacheCreate(kCFAllocatorDefault, nil, device, nil, &textureCache)
                self.textureCache = textureCache
                self.pipelineState = Self.makePipelineState(device: device)
            } else {
                self.pipelineState = nil
            }

            super.init()
        }

        func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

        func draw(in view: MTKView) {
            guard
                let commandQueue,
                let commandBuffer = commandQueue.makeCommandBuffer(),
                let renderPassDescriptor = view.currentRenderPassDescriptor,
                let drawable = view.currentDrawable
            else {
                return
            }

            let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: renderPassDescriptor)

            if
                let encoder,
                let pipelineState,
                let (lumaTexture, chromaTexture) = makeNV12Textures()
            {
                encoder.setRenderPipelineState(pipelineState)
                encoder.setFragmentTexture(lumaTexture, index: 0)
                encoder.setFragmentTexture(chromaTexture, index: 1)
                encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
                encoder.endEncoding()
            } else {
                encoder?.endEncoding()
            }

            commandBuffer.present(drawable)
            commandBuffer.commit()
        }

        private func makeNV12Textures() -> (MTLTexture, MTLTexture)? {
            guard
                let pixelBuffer = renderer.currentPixelBuffer(),
                let textureCache,
                CVPixelBufferGetPlaneCount(pixelBuffer) >= 2
            else {
                return nil
            }

            let lumaWidth = CVPixelBufferGetWidthOfPlane(pixelBuffer, 0)
            let lumaHeight = CVPixelBufferGetHeightOfPlane(pixelBuffer, 0)
            let chromaWidth = CVPixelBufferGetWidthOfPlane(pixelBuffer, 1)
            let chromaHeight = CVPixelBufferGetHeightOfPlane(pixelBuffer, 1)

            var lumaTextureRef: CVMetalTexture?
            var chromaTextureRef: CVMetalTexture?

            let lumaStatus = CVMetalTextureCacheCreateTextureFromImage(
                kCFAllocatorDefault,
                textureCache,
                pixelBuffer,
                nil,
                .r8Unorm,
                lumaWidth,
                lumaHeight,
                0,
                &lumaTextureRef
            )
            guard lumaStatus == kCVReturnSuccess else {
                return nil
            }

            let chromaStatus = CVMetalTextureCacheCreateTextureFromImage(
                kCFAllocatorDefault,
                textureCache,
                pixelBuffer,
                nil,
                .rg8Unorm,
                chromaWidth,
                chromaHeight,
                1,
                &chromaTextureRef
            )
            guard chromaStatus == kCVReturnSuccess else {
                return nil
            }

            guard
                let lumaTextureRef,
                let chromaTextureRef,
                let lumaTexture = CVMetalTextureGetTexture(lumaTextureRef),
                let chromaTexture = CVMetalTextureGetTexture(chromaTextureRef)
            else {
                return nil
            }

            return (lumaTexture, chromaTexture)
        }

        private static func makePipelineState(device: MTLDevice) -> MTLRenderPipelineState? {
            let source = """
            #include <metal_stdlib>
            using namespace metal;

            struct VertexOut {
                float4 position [[position]];
                float2 texCoord;
            };

            vertex VertexOut vertex_main(uint vertexID [[vertex_id]]) {
                const float2 positions[4] = {
                    float2(-1.0, -1.0),
                    float2( 1.0, -1.0),
                    float2(-1.0,  1.0),
                    float2( 1.0,  1.0)
                };
                const float2 texCoords[4] = {
                    float2(0.0, 1.0),
                    float2(1.0, 1.0),
                    float2(0.0, 0.0),
                    float2(1.0, 0.0)
                };

                VertexOut output;
                output.position = float4(positions[vertexID], 0.0, 1.0);
                output.texCoord = texCoords[vertexID];
                return output;
            }

            fragment float4 fragment_main(
                VertexOut input [[stage_in]],
                texture2d<float> lumaTexture [[texture(0)]],
                texture2d<float> chromaTexture [[texture(1)]]
            ) {
                constexpr sampler textureSampler(address::clamp_to_edge, filter::linear);
                float y = lumaTexture.sample(textureSampler, input.texCoord).r;
                float2 uv = chromaTexture.sample(textureSampler, input.texCoord).rg - float2(0.5, 0.5);

                float r = saturate(1.1643 * (y - 0.0625) + 1.5958 * uv.y);
                float g = saturate(1.1643 * (y - 0.0625) - 0.39173 * uv.x - 0.81290 * uv.y);
                float b = saturate(1.1643 * (y - 0.0625) + 2.017 * uv.x);
                return float4(r, g, b, 1.0);
            }
            """

            guard
                let library = try? device.makeLibrary(source: source, options: nil),
                let vertexFunction = library.makeFunction(name: "vertex_main"),
                let fragmentFunction = library.makeFunction(name: "fragment_main")
            else {
                return nil
            }

            let descriptor = MTLRenderPipelineDescriptor()
            descriptor.vertexFunction = vertexFunction
            descriptor.fragmentFunction = fragmentFunction
            descriptor.colorAttachments[0].pixelFormat = .bgra8Unorm

            return try? device.makeRenderPipelineState(descriptor: descriptor)
        }
    }
}
