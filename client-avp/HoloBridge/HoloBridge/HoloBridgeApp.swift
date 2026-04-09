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
    }
}
