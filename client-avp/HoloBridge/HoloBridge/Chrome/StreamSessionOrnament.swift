import SwiftUI

struct StreamSessionOrnament<Accessory: View>: View {
    @Environment(SessionManager.self) private var session

    let presentationMode: StreamPresentationMode
    private let accessory: () -> Accessory

    init(
        presentationMode: StreamPresentationMode,
        @ViewBuilder accessory: @escaping () -> Accessory
    ) {
        self.presentationMode = presentationMode
        self.accessory = accessory
    }

    var body: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 4) {
                Text(session.state.label)
                    .font(.headline)
                Text(session.videoRenderer.frameSizeDescription)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            Divider()
                .frame(height: 28)

            HStack(spacing: 8) {
                presentationButton(for: .window)
                presentationButton(for: .volume)
            }

            Divider()
                .frame(height: 28)

            Text("Frames \(session.videoRenderer.framesPresented)")
                .font(.caption)
                .foregroundStyle(.secondary)

            accessory()

            if let issue = session.videoRenderer.lastErrorMessage {
                Divider()
                    .frame(height: 28)
                Text(issue)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            #if DEBUG
            Divider()
                .frame(height: 28)

            Button {
                Task {
                    await session.simulateNetworkDrop()
                }
            } label: {
                Label("Drop Link", systemImage: "wifi.slash")
            }
            .buttonStyle(.bordered)
            #endif

            Button(role: .destructive) {
                Task {
                    await session.disconnect()
                }
            } label: {
                Label("Disconnect", systemImage: "link.badge.xmark")
            }
            .buttonStyle(.borderedProminent)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 12)
        .background(.regularMaterial, in: Capsule())
    }

    @ViewBuilder
    private func presentationButton(
        for mode: StreamPresentationMode
    ) -> some View {
        if mode == presentationMode {
            Button {
                session.switchPresentation(to: mode)
            } label: {
                Label(mode.label, systemImage: mode.systemImage)
            }
            .disabled(true)
            .buttonStyle(.borderedProminent)
        } else {
            Button {
                session.switchPresentation(to: mode)
            } label: {
                Label(mode.label, systemImage: mode.systemImage)
            }
            .buttonStyle(.bordered)
        }
    }
}

extension StreamSessionOrnament where Accessory == EmptyView {
    init(presentationMode: StreamPresentationMode) {
        self.init(presentationMode: presentationMode) {
            EmptyView()
        }
    }
}
