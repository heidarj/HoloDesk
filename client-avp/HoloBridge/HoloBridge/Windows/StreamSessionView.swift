import HoloBridgeClientCore
import SwiftUI

struct StreamSessionView: View {
    @Environment(SessionManager.self) private var session

    var body: some View {
        GeometryReader { geometry in
            let aspectRatio = resolvedAspectRatio
            let surfaceSize = fittedSurfaceSize(
                in: geometry.size,
                aspectRatio: aspectRatio
            )

            ZStack {
                LinearGradient(
                    colors: [
                        Color(red: 0.06, green: 0.07, blue: 0.09),
                        Color(red: 0.01, green: 0.01, blue: 0.02),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )

                streamSurface(size: surfaceSize)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(24)
        }
        .ornament(
            visibility: .visible,
            attachmentAnchor: .scene(.bottom),
            contentAlignment: .center
        ) {
            StreamSessionOrnament(presentationMode: .window)
                .onHover { hovering in
                    session.setOrnamentInteraction(active: hovering)
                }
        }
        .onAppear {
            session.noteStreamPresentationVisibility(true)
        }
        .onDisappear {
            session.noteStreamPresentationVisibility(false)
        }
    }

    @ViewBuilder
    private func streamSurface(size: CGSize) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 28)
                .fill(Color.black.opacity(0.92))

            VideoDisplayView(renderer: session.videoRenderer)

            RemoteInputCaptureView(
                session: session,
                videoPixelSize: resolvedVideoSize
            )

            pointerOverlay(in: size)

            if session.videoRenderer.isAwaitingFrame {
                VStack(spacing: 10) {
                    ProgressView()
                        .progressViewStyle(.circular)
                    Text(session.videoRenderer.statusMessage)
                        .font(.headline)
                    Text("The QUIC session is connected. Waiting for the next decoded H.264 frame.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 360)
                }
                .padding(24)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18))
            }
        }
        .frame(width: size.width, height: size.height)
        .clipShape(RoundedRectangle(cornerRadius: 28))
        .overlay {
            RoundedRectangle(cornerRadius: 28)
                .strokeBorder(Color.white.opacity(0.08), lineWidth: 1)
        }
        .allowsWindowActivationEvents(false)
    }

    @ViewBuilder
    private func pointerOverlay(in size: CGSize) -> some View {
        if
            let pointerImage = session.videoRenderer.pointerImage,
            session.videoRenderer.pointerVisible,
            session.videoRenderer.videoFrameWidth > 0,
            session.videoRenderer.videoFrameHeight > 0
        {
            let scaleX = size.width / CGFloat(session.videoRenderer.videoFrameWidth)
            let scaleY = size.height / CGFloat(session.videoRenderer.videoFrameHeight)
            let displayWidth = max(CGFloat(session.videoRenderer.pointerWidth) * scaleX, 1)
            let displayHeight = max(CGFloat(session.videoRenderer.pointerHeight) * scaleY, 1)
            let centerX =
                (CGFloat(session.videoRenderer.pointerX - session.videoRenderer.pointerHotspotX) * scaleX)
                + (displayWidth / 2)
            let centerY =
                (CGFloat(session.videoRenderer.pointerY - session.videoRenderer.pointerHotspotY) * scaleY)
                + (displayHeight / 2)

            Image(uiImage: pointerImage)
                .resizable()
                .interpolation(.none)
                .frame(width: displayWidth, height: displayHeight)
                .position(x: centerX, y: centerY)
                .allowsHitTesting(false)
        }
    }

    private var resolvedVideoSize: CGSize {
        if session.videoRenderer.videoFrameWidth > 0, session.videoRenderer.videoFrameHeight > 0 {
            return CGSize(
                width: session.videoRenderer.videoFrameWidth,
                height: session.videoRenderer.videoFrameHeight
            )
        }
        return CGSize(width: 16, height: 9)
    }

    private var resolvedAspectRatio: CGFloat {
        let videoSize = resolvedVideoSize
        guard videoSize.height > 0 else {
            return 16.0 / 9.0
        }
        return videoSize.width / videoSize.height
    }

    private func fittedSurfaceSize(
        in containerSize: CGSize,
        aspectRatio: CGFloat
    ) -> CGSize {
        guard containerSize.width > 0, containerSize.height > 0, aspectRatio > 0 else {
            return .zero
        }

        let width = containerSize.width
        let height = containerSize.height
        let containerAspect = width / height
        if containerAspect > aspectRatio {
            let fittedHeight = height
            return CGSize(width: fittedHeight * aspectRatio, height: fittedHeight)
        }

        let fittedWidth = width
        return CGSize(width: fittedWidth, height: fittedWidth / aspectRatio)
    }
}

#if DEBUG
#Preview {
    StreamSessionView()
        .environment(
            SessionManager(
                preview: .connected(userDisplayName: "Preview User"),
                presentationMode: .window
            )
        )
}
#endif
