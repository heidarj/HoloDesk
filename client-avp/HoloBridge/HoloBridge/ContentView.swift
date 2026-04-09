import HoloBridgeClientCore
import SwiftUI

struct ContentView: View {
    @Environment(SessionManager.self) private var session
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow

    @State private var hostAddress = "127.0.0.1"
    @State private var port = "4433"

    var body: some View {
        @Bindable var session = session

        VStack(spacing: 24) {
            Text("HoloBridge")
                .font(.largeTitle)
                .fontWeight(.bold)

            statusView

            if session.streamWindowRequested {
                connectedUtilityView
            } else {
                connectionForm
            }
        }
        .padding(40)
        .frame(minWidth: 520, minHeight: 420)
        .onAppear {
            synchronizeStreamWindow(session.streamWindowRequested)
        }
        .onChange(of: session.streamWindowRequested) { _, requested in
            synchronizeStreamWindow(requested)
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

            Button {
                let portNum = UInt16(port) ?? 4433
                session.connect(host: hostAddress, port: portNum)
            } label: {
                Label(connectLabel, systemImage: connectSystemImage)
                    .frame(maxWidth: 250)
            }
            .buttonStyle(.borderedProminent)
            .disabled(isConnecting)

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
                Label("Stream window active", systemImage: "rectangle.inset.filled.and.person.filled")
                    .font(.headline)
                Text("The desktop stream now runs in its own window. This utility window stays available for reconnect, link diagnostics, and disconnect.")
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

    private var connectLabel: String {
        switch session.authMode {
        case .apple: return "Sign In and Connect"
        case .test: return "Connect"
        case .none: return "Connect (No Auth)"
        }
    }

    private var connectSystemImage: String {
        session.authMode == .apple ? "person.badge.key" : "link"
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

    private func synchronizeStreamWindow(_ requested: Bool) {
        if requested {
            openWindow(id: "stream-session")
        } else {
            dismissWindow(id: "stream-session")
        }
    }
}

#Preview(windowStyle: .automatic) {
    ContentView()
        .environment(SessionManager(authMode: .test))
}
