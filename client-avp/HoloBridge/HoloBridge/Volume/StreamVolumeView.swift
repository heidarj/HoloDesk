import Foundation
import HoloBridgeClientCore
import RealityKit
import SwiftUI
import UIKit

@MainActor
final class CurvedDisplaySceneController {
    private let surface = RealityKitVideoTextureSurface()
    private let rootEntity = Entity()
    private let screenEntity = ModelEntity()

    private var isInstalled = false
    private var appliedMeshParameters: CurvedPanelMeshFactory.Parameters?
    private var appliedTextureSize = CGSize.zero

    init() {
        rootEntity.position = [0, 0, -0.7]
        rootEntity.addChild(screenEntity)
    }

    func install(
        in content: inout RealityViewContent,
        renderer: VideoRenderer
    ) async {
        guard !isInstalled else {
            return
        }

        content.add(rootEntity)
        surface.bind(renderer: renderer)
        surface.start()
        isInstalled = true
    }

    func update(
        renderer: VideoRenderer,
        meshParameters: CurvedPanelMeshFactory.Parameters,
        preferredTextureSize: CGSize
    ) {
        surface.bind(renderer: renderer)

        if appliedMeshParameters != meshParameters {
            if let mesh = try? CurvedPanelMeshFactory.makeMesh(parameters: meshParameters) {
                let existingMaterials = screenEntity.model?.materials ?? [UnlitMaterial(color: UIColor.black)]
                screenEntity.model = ModelComponent(mesh: mesh, materials: existingMaterials)
                appliedMeshParameters = meshParameters
            }
        }

        if preferredTextureSize != appliedTextureSize || (screenEntity.model?.materials.isEmpty ?? true) {
            if let material = surface.material(preferredTextureSize: preferredTextureSize) {
                if screenEntity.model == nil {
                    if let mesh = try? CurvedPanelMeshFactory.makeMesh(parameters: meshParameters) {
                        screenEntity.model = ModelComponent(mesh: mesh, materials: [material])
                    }
                } else {
                    screenEntity.model?.materials = [material]
                }
                appliedTextureSize = preferredTextureSize
            }
        }
    }

    func stop() {
        surface.stop()
    }
}

struct StreamVolumeView: View {
    @Environment(SessionManager.self) private var session

    @State private var screenRadiusMeters = 2.4
    @State private var sceneController = CurvedDisplaySceneController()

    var body: some View {
        presentationBody
        .ornament(
            visibility: .visible,
            attachmentAnchor: .scene(.bottom),
            contentAlignment: .center
        ) {
            StreamSessionOrnament(presentationMode: .volume) {
                Divider()
                    .frame(height: 28)

                VStack(alignment: .leading, spacing: 4) {
                    Text("Curve")
                        .font(.caption.weight(.semibold))
                    HStack(spacing: 10) {
                        Text(radiusDescription)
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                        Slider(value: $screenRadiusMeters, in: 0.8...4.0)
                            .frame(width: 180)
                    }
                    Text("Arc \(meshParameters.totalArcAngleDegrees, specifier: "%.0f")°")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            .onHover { hovering in
                session.setOrnamentInteraction(active: hovering)
            }
        }
        .onAppear {
            session.noteStreamPresentationVisibility(true)
        }
        .onDisappear {
            session.noteStreamPresentationVisibility(false)
            sceneController.stop()
        }
    }

    @ViewBuilder
    private var presentationBody: some View {
        if Self.isRunningPreview {
            previewCanvasBody
        } else {
            runtimeVolumeBody
        }
    }

    private var runtimeVolumeBody: some View {
        RealityView { content in
            await sceneController.install(
                in: &content,
                renderer: session.videoRenderer
            )
        } update: { _ in
            sceneController.update(
                renderer: session.videoRenderer,
                meshParameters: meshParameters,
                preferredTextureSize: preferredTextureSize
            )
        } placeholder: {
            ProgressView()
                .progressViewStyle(.circular)
        }
    }

    private var previewCanvasBody: some View {
        GeometryReader { geometry in
            let surfaceSize = fittedPreviewSurfaceSize(in: geometry.size)

            ZStack {
                LinearGradient(
                    colors: [
                        Color(red: 0.03, green: 0.04, blue: 0.06),
                        Color(red: 0.01, green: 0.01, blue: 0.02),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )

                VStack {
                    Spacer(minLength: 24)

                    previewSurface(size: surfaceSize)

                    Spacer(minLength: max(geometry.size.height * 0.16, 72))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .padding(.horizontal, 28)
            }
        }
    }

    private func previewSurface(size: CGSize) -> some View {
        ZStack(alignment: .topLeading) {
            RoundedRectangle(cornerRadius: 36)
                .fill(Color.black.opacity(0.92))

            VideoDisplayView(renderer: session.videoRenderer)

            RoundedRectangle(cornerRadius: 36)
                .strokeBorder(Color.white.opacity(0.08), lineWidth: 1)

            Text("Canvas Preview")
                .font(.caption.weight(.semibold))
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(.ultraThinMaterial, in: Capsule())
                .padding(18)
        }
        .frame(width: size.width, height: size.height)
        .clipShape(RoundedRectangle(cornerRadius: 36))
        .overlay {
            HStack {
                LinearGradient(
                    colors: [Color.black.opacity(0.26), .clear],
                    startPoint: .leading,
                    endPoint: .trailing
                )
                LinearGradient(
                    colors: [.clear, Color.black.opacity(0.26)],
                    startPoint: .leading,
                    endPoint: .trailing
                )
            }
            .allowsHitTesting(false)
        }
        .rotation3DEffect(
            .degrees(63),
            axis: (x: 1, y: 0, z: 0)
        )
        .rotation3DEffect(
            .degrees(-7),
            axis: (x: 0, y: 1, z: 0)
        )
        .shadow(color: .black.opacity(0.42), radius: 36, y: 22)
    }

    private var preferredTextureSize: CGSize {
        let width = max(session.videoRenderer.videoFrameWidth, 1280)
        let height = max(session.videoRenderer.videoFrameHeight, 720)
        return CGSize(width: width, height: height)
    }

    private var meshParameters: CurvedPanelMeshFactory.Parameters {
        CurvedPanelMeshFactory.Parameters(
            aspectRatio: Float(resolvedAspectRatio),
            panelHeightMeters: 0.72,
            radiusMeters: Float(screenRadiusMeters),
            horizontalSegments: 96,
            verticalSegments: 24
        )
    }

    private var resolvedAspectRatio: CGFloat {
        let width = CGFloat(max(session.videoRenderer.videoFrameWidth, 16))
        let height = CGFloat(max(session.videoRenderer.videoFrameHeight, 9))
        return width / height
    }

    private var radiusDescription: String {
        String(format: "%.1fm", screenRadiusMeters)
    }

    private func fittedPreviewSurfaceSize(
        in containerSize: CGSize
    ) -> CGSize {
        guard containerSize.width > 0, containerSize.height > 0 else {
            return .zero
        }

        let maxWidth = containerSize.width * 0.78
        let maxHeight = containerSize.height * 0.46
        let aspectRatio = resolvedAspectRatio
        let width = min(maxWidth, maxHeight * aspectRatio)
        return CGSize(width: width, height: width / aspectRatio)
    }

    private static var isRunningPreview: Bool {
        ProcessInfo.processInfo.environment["XCODE_RUNNING_FOR_PREVIEWS"] == "1"
    }
}

#if DEBUG
#Preview(windowStyle: .volumetric) {
    StreamVolumeView()
        .environment(
            SessionManager(
                preview: .connected(userDisplayName: "Preview User"),
                presentationMode: .volume
            )
        )
}
#endif
