import SwiftUI

@main
struct HoloBridgeApp: App {
    @State private var session = SessionManager()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(session)
        }

        WindowGroup(id: "stream-session") {
            StreamSessionView()
                .environment(session)
        }

        WindowGroup(id: "stream-volume") {
            StreamVolumeView()
                .environment(session)
        }
        .windowStyle(.volumetric)
        .defaultSize(width: 1.85, height: 1.1, depth: 1.2, in: .meters)
    }
}
