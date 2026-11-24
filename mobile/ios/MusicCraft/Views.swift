import SwiftUI
import UniformTypeIdentifiers

struct RootView: View {
    @EnvironmentObject var library: Library
    @EnvironmentObject var player: Player
    @State private var picking = false

    var body: some View {
        Group {
            if !library.ready {
                ProgressView()
            } else if library.folder == nil {
                Welcome { picking = true }
            } else {
                TabView {
                    NavigationStack { AlbumsView(pick: { picking = true }) }
                        .tabItem { Label("Albums", systemImage: "square.stack") }
                    NavigationStack { SongsView() }
                        .tabItem { Label("Songs", systemImage: "music.note") }
                    NavigationStack { PlaylistsView() }
                        .tabItem { Label("Playlists", systemImage: "music.note.list") }
                }
                .safeAreaInset(edge: .bottom) { MiniPlayer().padding(.bottom, 49) }
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result { library.choose(url) }
        }
        .alert(
            "Something went wrong",
            isPresented: Binding(get: { library.error != nil }, set: { if !$0 { library.error = nil } }),
            actions: { Button("Close") { library.error = nil } },
            message: { Text(library.error ?? "") }
        )
    }
}

struct Welcome: View {
    let choose: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Text("MusicCraft").font(.largeTitle)
            Text("Choose your MusicCraft folder: the one with Music and Library inside, that your computers share. It can be in iCloud Drive, in a sync app that offers its folders in Files, or on this device.")
                .multilineTextAlignment(.center)
            Button("Choose the folder", action: choose).buttonStyle(.borderedProminent)
        }
        .padding(24)
    }
}

/// Reload and change-folder buttons, on every list.
struct LibraryToolbar: ToolbarContent {
    @EnvironmentObject var library: Library
    let pick: (() -> Void)?

    var body: some ToolbarContent {
        ToolbarItemGroup(placement: .navigationBarTrailing) {
            if library.busy { ProgressView() }
            Button { library.reload() } label: { Label("Read again", systemImage: "arrow.clockwise") }
            if let pick { Button("Change folder", action: pick) }
        }
    }
}

struct AlbumsView: View {
    @EnvironmentObject var library: Library
    let pick: () -> Void

    var body: some View {
        List {
            if !library.warnings.isEmpty {
                Text("Some files could not be read: \(library.warnings.joined(separator: "; "))").font(.footnote)
            }
            if library.albums.isEmpty && !library.busy {
                Text("No songs yet. Add music on a computer, or put files in the Music folder.")
            }
            ForEach(library.albums) { album in
                NavigationLink {
                    SongList(ids: album.songs).navigationTitle(album.title)
                } label: {
                    VStack(alignment: .leading) {
                        Text(album.title.isEmpty ? String(localized: "Unknown album") : album.title)
                        Text([album.artist, album.year > 0 ? String(album.year) : ""].filter { !$0.isEmpty }.joined(separator: " · "))
                            .font(.footnote).foregroundStyle(.secondary)
                    }
                }
            }
        }
        .navigationTitle("Albums")
        .toolbar { LibraryToolbar(pick: pick) }
    }
}

struct SongsView: View {
    @EnvironmentObject var library: Library
    @State private var query = ""

    var body: some View {
        let words = query.lowercased().split(separator: " ")
        let ids = library.order.filter { id in
            guard let s = library.songs[id] else { return false }
            let text = "\(s.title) \(s.artist) \(s.album)".lowercased()
            return words.allSatisfy { text.contains($0) }
        }
        SongList(ids: ids)
            .searchable(text: $query)
            .navigationTitle("Songs")
            .toolbar { LibraryToolbar(pick: nil) }
    }
}

struct PlaylistsView: View {
    @EnvironmentObject var library: Library

    var body: some View {
        List(library.playlists) { p in
            NavigationLink {
                PlaylistView(playlist: p)
            } label: {
                VStack(alignment: .leading) {
                    Text(p.name)
                    if p.smart { Text("Smart playlist").font(.footnote).foregroundStyle(.secondary) }
                }
            }
        }
        .navigationTitle("Playlists")
        .toolbar { LibraryToolbar(pick: nil) }
    }
}

struct PlaylistView: View {
    @EnvironmentObject var library: Library
    let playlist: PlaylistInfo
    @State private var ids: [String] = []

    var body: some View {
        SongList(ids: ids)
            .navigationTitle(playlist.name)
            .task(id: library.songs.count) { ids = await library.playlistSongs(playlist.id) }
    }
}

struct SongList: View {
    @EnvironmentObject var library: Library
    @EnvironmentObject var player: Player
    let ids: [String]

    var body: some View {
        List {
            ForEach(Array(ids.enumerated()), id: \.offset) { i, id in
                if let song = library.songs[id] {
                    SongRow(song: song) { player.play(ids, from: i) }
                }
            }
        }
        .listStyle(.plain)
    }
}

struct SongRow: View {
    @EnvironmentObject var library: Library
    let song: Song
    let play: () -> Void

    var body: some View {
        HStack {
            Button(action: play) {
                VStack(alignment: .leading) {
                    Text(song.title).lineLimit(1)
                    Text(subtitle).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(!song.here)
            .opacity(song.here ? 1 : 0.5)
            if song.rating > 0 {
                Text(String(repeating: "★", count: song.rating)).font(.footnote)
                    .accessibilityLabel(Text("\(song.rating) stars"))
            }
            Button {
                library.setLoved(song.id, !song.loved)
            } label: {
                Image(systemName: song.loved ? "heart.fill" : "heart")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(Text("Love"))
            .accessibilityValue(Text(song.loved ? "Loved" : ""))
        }
        .contextMenu {
            ForEach(0...5, id: \.self) { n in
                Button(n == 0 ? String(localized: "No rating") : String(localized: "\(n) stars")) { library.setRating(song.id, n) }
            }
        }
    }

    private var subtitle: String {
        let parts = [song.artist, song.album].filter { !$0.isEmpty }.joined(separator: " · ")
        return song.here ? parts : "\(parts) · \(String(localized: "Not on this device"))"
    }
}

struct MiniPlayer: View {
    @EnvironmentObject var player: Player

    var body: some View {
        if let song = player.current {
            HStack {
                Text("\(song.title) · \(song.artist)").lineLimit(1)
                Spacer()
                Button { player.previous() } label: { Image(systemName: "backward.fill") }
                    .accessibilityLabel(Text("Previous"))
                Button { player.toggle() } label: { Image(systemName: player.playing ? "pause.fill" : "play.fill") }
                    .accessibilityLabel(Text(player.playing ? "Pause" : "Play"))
                Button { player.next() } label: { Image(systemName: "forward.fill") }
                    .accessibilityLabel(Text("Next"))
            }
            .buttonStyle(.borderless)
            .padding(.horizontal)
            .padding(.vertical, 8)
            .background(.bar)
        }
    }
}
