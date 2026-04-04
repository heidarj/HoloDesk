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
                connectedActions
            }
        }
        .padding(40)
        .frame(minWidth: 400)
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
                    .frame(maxWidth: 200)

                TextField("Port", text: $port)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 80)
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
