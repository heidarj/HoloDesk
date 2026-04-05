import SwiftUI

struct ContentView: View {
    @Environment(SessionManager.self) private var session

    @State private var hostAddress = "127.0.0.1"
    @State private var port = "4433"

    var body: some View {
        @Bindable var session = session

        VStack(spacing: 24) {
            Text("HoloBridge")
                .font(.largeTitle)
                .fontWeight(.bold)

            statusView

            if !session.state.isConnected {
                connectionForm
            } else {
                connectedSessionView
            }
        }
        .padding(40)
        .frame(minWidth: 520, minHeight: 680)
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
                Task {
                    await session.connect(host: hostAddress, port: portNum)
                }
            } label: {
                Label(connectLabel, systemImage: connectSystemImage)
                    .frame(maxWidth: 250)
            }
            .buttonStyle(.borderedProminent)
            .disabled(isConnecting)
        }
    }

    @ViewBuilder
    private var connectedSessionView: some View {
        VStack(spacing: 18) {
            ZStack {
                RoundedRectangle(cornerRadius: 24)
                    .fill(
                        LinearGradient(
                            colors: [
                                Color(red: 0.06, green: 0.07, blue: 0.09),
                                Color(red: 0.01, green: 0.01, blue: 0.02),
                            ],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )

                VideoDisplayView(renderer: session.videoRenderer)
                    .padding(10)

                if session.videoRenderer.isAwaitingFrame {
                    VStack(spacing: 10) {
                        ProgressView()
                            .progressViewStyle(.circular)
                        Text(session.videoRenderer.statusMessage)
                            .font(.headline)
                        Text("The QUIC session is connected. Waiting for the first decoded H.264 frame.")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 360)
                    }
                    .padding(24)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18))
                }
            }
            .frame(minWidth: 820, idealWidth: 920, maxWidth: 980, minHeight: 420, idealHeight: 520)
            .overlay(alignment: .topLeading) {
                videoBadge
                    .padding(18)
            }

            if let issue = session.videoRenderer.lastErrorMessage {
                Text(issue)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: 820, alignment: .leading)
            }

            connectedActions
        }
    }

    @ViewBuilder
    private var videoBadge: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(session.videoRenderer.statusMessage)
                .font(.headline)
            Text(session.videoRenderer.frameSizeDescription)
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Text("Frames presented: \(session.videoRenderer.framesPresented)")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14))
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
        session.authMode == .apple ? "Sign In and Connect" : "Connect"
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
        case .connected: return .green
        case .error: return .red
        }
    }
}

#Preview(windowStyle: .automatic) {
    ContentView()
        .environment(SessionManager(authMode: .test))
}
