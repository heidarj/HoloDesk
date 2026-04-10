import HoloBridgeClientCore
import SwiftUI

struct ContentView: View {
    @Environment(SessionManager.self) private var session
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow

    @State private var hostAddress = "192.168.2.100"
    @State private var port = "4433"

    var body: some View {
        @Bindable var session = session

        VStack(spacing: 24) {
            Text("HoloBridge")
                .font(.largeTitle)
                .fontWeight(.bold)

            statusView

            if session.streamWindowRequested || session.streamVolumeRequested {
                connectedUtilityView
            } else {
                connectionForm
            }
        }
        .padding(40)
        .frame(minWidth: 520, minHeight: 420)
        .onAppear {
            synchronizeStreamPresentations()
        }
        .onChange(of: session.streamWindowRequested) { _, requested in
            synchronizeStreamPresentation(id: "stream-session", requested: requested)
        }
        .onChange(of: session.streamVolumeRequested) { _, requested in
            synchronizeStreamPresentation(id: "stream-volume", requested: requested)
        }
    }

    @ViewBuilder
    private var statusView: some View {
        HStack(spacing: 12) {
            Circle()
                .fill(statusColor)
                .frame(width: 12, height: 12)
            Text(session.state.label)
                .font(.headline)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
    }

    @ViewBuilder
    private var connectionForm: some View {
        @Bindable var session = session

        VStack(spacing: 16) {
            #if DEBUG
            Picker("Auth Mode", selection: $session.authMode) {
                ForEach(AuthMode.allCases) { mode in
                    Text(mode.label).tag(mode)
                }
            }
            .pickerStyle(.segmented)
            .frame(maxWidth: 250)
            #endif

            HStack(spacing: 12) {
                TextField("Host", text: $hostAddress)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 220)

                TextField("Port", text: $port)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 90)
            }

            HStack(spacing: 12) {
                Button {
                    connect(using: .window)
                } label: {
                    Label("Connect Window", systemImage: StreamPresentationMode.window.systemImage)
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .disabled(isConnecting)

                Button {
                    connect(using: .volume)
                } label: {
                    Label("Connect Volume", systemImage: StreamPresentationMode.volume.systemImage)
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .disabled(isConnecting)
            }
            .frame(maxWidth: 420)

            Text(connectExplanation)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)

            if isConnecting {
                Button(role: .destructive) {
                    Task {
                        await session.cancelConnection()
                    }
                } label: {
                    Label("Cancel", systemImage: "xmark.circle")
                        .frame(maxWidth: 250)
                }
                .buttonStyle(.bordered)
            }
        }
    }

    @ViewBuilder
    private var connectedUtilityView: some View {
        VStack(spacing: 18) {
            VStack(alignment: .leading, spacing: 12) {
                Label(
                    "Stream \(session.activePresentationMode.label.lowercased()) active",
                    systemImage: session.activePresentationMode.systemImage
                )
                    .font(.headline)
                Text(session.activePresentationMode.utilityDescription)
                    .foregroundStyle(.secondary)
                Text("Video: \(session.videoRenderer.frameSizeDescription)")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                Text("Frames presented: \(session.videoRenderer.framesPresented)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20))

            if let issue = session.videoRenderer.lastErrorMessage {
                Text(issue)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            connectedActions
        }
    }

    @ViewBuilder
    private var connectedActions: some View {
        VStack(spacing: 12) {
            HStack(spacing: 12) {
                presentationSwitcherButton(
                    for: .window,
                    title: "Show Window"
                )
                presentationSwitcherButton(
                    for: .volume,
                    title: "Show Volume"
                )
            }
            .frame(maxWidth: 420)

            #if DEBUG
            Button {
                Task {
                    await session.simulateNetworkDrop()
                }
            } label: {
                Label("Simulate Network Drop", systemImage: "wifi.slash")
                    .frame(maxWidth: 250)
            }
            .buttonStyle(.bordered)
            #endif

            Button(role: .destructive) {
                Task {
                    await session.disconnect()
                }
            } label: {
                Label("Disconnect", systemImage: "link.badge.xmark")
                    .frame(maxWidth: 250)
            }
            .buttonStyle(.bordered)
        }
    }

    private var connectExplanation: String {
        switch session.authMode {
        case .apple:
            return "Each presentation signs in first, then opens either the standard stream window or the new RealityKit volume."
        case .test:
            return "Test auth is enabled for local iteration. Choose whether the stream opens in the standard window or the new RealityKit volume."
        case .none:
            return "No host auth is being requested. Choose whether the stream opens in the standard window or the new RealityKit volume."
        }
    }

    private var isConnecting: Bool {
        switch session.state {
        case .connecting, .authenticating, .resuming:
            return true
        default:
            return false
        }
    }

    private var statusColor: Color {
        switch session.state {
        case .disconnected: return .gray
        case .connecting, .authenticating, .resuming: return .yellow
        case .connected(_): return .green
        case .error(_): return .red
        }
    }

    private func synchronizeStreamPresentations() {
        synchronizeStreamPresentation(id: "stream-session", requested: session.streamWindowRequested)
        synchronizeStreamPresentation(id: "stream-volume", requested: session.streamVolumeRequested)
    }

    private func synchronizeStreamPresentation(
        id: String,
        requested: Bool
    ) {
        if requested {
            openWindow(id: id)
        } else {
            dismissWindow(id: id)
        }
    }

    private func connect(using presentationMode: StreamPresentationMode) {
        let portNum = UInt16(port) ?? 4433
        session.connect(host: hostAddress, port: portNum, presentationMode: presentationMode)
    }

    @ViewBuilder
    private func presentationSwitcherButton(
        for presentationMode: StreamPresentationMode,
        title: String
    ) -> some View {
        if session.activePresentationMode == presentationMode {
            Button {
                session.switchPresentation(to: presentationMode)
            } label: {
                Label(title, systemImage: presentationMode.systemImage)
                    .frame(maxWidth: .infinity)
            }
            .disabled(true)
            .buttonStyle(.borderedProminent)
        } else {
            Button {
                session.switchPresentation(to: presentationMode)
            } label: {
                Label(title, systemImage: presentationMode.systemImage)
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
        }
    }
}

#Preview(windowStyle: .automatic) {
    ContentView()
        .environment(SessionManager(authMode: .test))
}
