import SwiftUI

@main
struct MusicCraftApp: App {
    @StateObject private var library = Library()
    @StateObject private var player = Player()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(library)
                .environmentObject(player)
                .onAppear { player.library = library }
        }
        .onChange(of: phase) { now in
            if now == .active { library.reload() }
        }
    }
}
