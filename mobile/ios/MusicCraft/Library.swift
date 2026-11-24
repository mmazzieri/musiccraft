import Foundation
import UIKit

struct Song: Identifiable, Equatable {
    let id: String
    let title: String
    let artist: String
    let album: String
    let durationMs: Int
    let loved: Bool
    let rating: Int
    /// The song's file is on this phone.
    let here: Bool
    let path: String
}

struct Album: Identifiable, Equatable {
    var id: String { key }
    let key: String
    let title: String
    let artist: String
    let year: Int
    let songs: [String]
}

struct PlaylistInfo: Identifiable, Equatable {
    let id: String
    let name: String
    let smart: Bool
}

/// The library folder and its catalogue. File work runs on a queue of its own, one task at a time;
/// what the screens show is published on the main thread.
@MainActor
final class Library: ObservableObject {
    @Published private(set) var ready = false
    @Published private(set) var folder: URL?
    @Published private(set) var busy = false
    @Published private(set) var songs: [String: Song] = [:]
    @Published private(set) var order: [String] = []
    @Published private(set) var albums: [Album] = []
    @Published private(set) var playlists: [PlaylistInfo] = []
    @Published private(set) var warnings: [String] = []
    @Published var error: String?

    private let work = DispatchQueue(label: "musiccraft.library")
    private let defaults = UserDefaults.standard
    /// Files below Music/, by their path there.
    private var files: [String: URL] = [:]
    /// This phone's own files that are damaged in the shared folder and could not be set aside.
    private var blocked: Set<String> = []

    init() {
        let device = Self.device()
        let name = UIDevice.current.name
        let saved = defaults.data(forKey: "folder")
        run { lib in
            try Core.ask("open", ["device": device, "device_name": name])
            if let saved {
                var stale = false
                let url = try URL(resolvingBookmarkData: saved, bookmarkDataIsStale: &stale)
                if url.startAccessingSecurityScopedResource() {
                    lib.setFolder(url)
                    try lib.load(url)
                }
            }
        } then: { $0.ready = true }
    }

    /// Uses a folder the person picked.
    func choose(_ url: URL) {
        guard url.startAccessingSecurityScopedResource() else {
            error = String(localized: "The folder cannot be opened.")
            return
        }
        if let data = try? url.bookmarkData() { defaults.set(data, forKey: "folder") }
        folder?.stopAccessingSecurityScopedResource()
        folder = url
        run { lib in try lib.load(url) }
    }

    /// Reads the catalogue and the music folder again: other computers may have changed them.
    func reload() {
        guard let url = folder else { return }
        run { lib in try lib.load(url) }
    }

    func fileURL(_ song: Song) -> URL? { files[song.path] }

    func setLoved(_ id: String, _ loved: Bool) { change("set_loved", ["id": id, "loved": loved]) }
    func setRating(_ id: String, _ stars: Int) { change("set_rating", ["id": id, "stars": stars]) }
    func played(_ id: String) { change("played", ["id": id]) }
    func skipped(_ id: String) { change("skipped", ["id": id]) }

    func playlistSongs(_ id: String) async -> [String] {
        await withCheckedContinuation { done in
            work.async {
                let ids = (try? Core.ask("playlist_songs", ["id": id])) as? [[String: Any]] ?? []
                done.resume(returning: ids.compactMap { $0["id"] as? String })
            }
        }
    }

    private func change(_ op: String, _ fields: [String: Any]) {
        guard let url = folder else { return }
        run { lib in
            try Core.ask(op, fields)
            try lib.save(url)
            try lib.refresh()
        }
    }

    // ---- on the work queue ----

    /// Runs `body` on the work queue with a view of this library that is safe there, then `then` on
    /// the main thread.
    private func run(_ body: @escaping (Worker) throws -> Void, then: @escaping (Library) -> Void = { _ in }) {
        busy = true
        let worker = Worker(files: files, blocked: blocked)
        work.async {
            var failure: String?
            do { try body(worker) } catch { failure = error.localizedDescription }
            Task { @MainActor in
                self.busy = false
                if let failure { self.error = failure }
                worker.publish(to: self)
                then(self)
            }
        }
    }

    fileprivate func take(_ w: Worker) {
        if let url = w.newFolder { folder = url }
        files = w.files
        blocked = w.blocked
        if let s = w.songs {
            songs = s.songs
            order = s.order
            albums = s.albums
            playlists = s.playlists
        }
        if let warnings = w.warnings { self.warnings = warnings }
    }

    private static func device() -> String {
        if let id = UserDefaults.standard.string(forKey: "device") { return id }
        let id = UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
        UserDefaults.standard.set(id, forKey: "device")
        return id
    }
}

/// The library's file work, done away from the main thread.
private final class Worker {
    struct Lists {
        var songs: [String: Song] = [:]
        var order: [String] = []
        var albums: [Album] = []
        var playlists: [PlaylistInfo] = []
    }

    var files: [String: URL]
    var blocked: Set<String>
    var songs: Lists?
    var warnings: [String]?
    var newFolder: URL?
    private let manager = FileManager.default

    init(files: [String: URL], blocked: Set<String>) {
        self.files = files
        self.blocked = blocked
    }

    func setFolder(_ url: URL) { newFolder = url }

    @MainActor func publish(to library: Library) { library.take(self) }

    private func backups() -> URL {
        let dir = manager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("library")
        try? manager.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    private func read(_ url: URL) throws -> String {
        var result: Result<String, Error> = .failure(CoreError(message: "not read"))
        var coordination: NSError?
        NSFileCoordinator().coordinate(readingItemAt: url, options: [], error: &coordination) { at in
            result = Result { String(decoding: try Data(contentsOf: at), as: UTF8.self) }
        }
        if let coordination { throw coordination }
        return try result.get()
    }

    private func write(_ text: String, to url: URL) throws {
        var failure: Error?
        var coordination: NSError?
        NSFileCoordinator().coordinate(writingItemAt: url, options: .forReplacing, error: &coordination) { at in
            do { try Data(text.utf8).write(to: at, options: .atomic) } catch { failure = error }
        }
        if let coordination { throw coordination }
        if let failure { throw failure }
    }

    func load(_ root: URL) throws {
        let libraryDir = root.appendingPathComponent("Library", isDirectory: true)
        let musicDir = root.appendingPathComponent("Music", isDirectory: true)
        try manager.createDirectory(at: libraryDir, withIntermediateDirectories: true)
        try manager.createDirectory(at: musicDir, withIntermediateDirectories: true)
        let names = (try? manager.contentsOfDirectory(at: libraryDir, includingPropertiesForKeys: nil)) ?? []
        var shared: [[String: Any]] = []
        for url in names where url.pathExtension == "json" {
            if let text = try? read(url) { shared.append(["name": url.lastPathComponent, "text": text]) }
        }
        var kept: [[String: Any]] = []
        for url in (try? manager.contentsOfDirectory(at: backups(), includingPropertiesForKeys: nil)) ?? []
        where url.pathExtension == "json" {
            if let text = try? String(contentsOf: url, encoding: .utf8) { kept.append(["name": url.lastPathComponent, "text": text]) }
        }
        let answer = try Core.ask("load", ["files": shared, "backups": kept]) as? [String: Any] ?? [:]
        // A damaged file of this phone's own is renamed, so that writing it again loses nothing.
        var still: Set<String> = []
        for name in answer["set_aside"] as? [String] ?? [] {
            let from = libraryDir.appendingPathComponent(name)
            let to = libraryDir.appendingPathComponent("\(name).damaged-\(Int(Date().timeIntervalSince1970 * 1000))")
            if (try? manager.moveItem(at: from, to: to)) == nil { still.insert(name) }
        }
        blocked = still
        files = music(musicDir)
        let found = files.map { path, url -> [String: Any] in
            let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            return ["path": path, "size": size]
        }
        try Core.ask("found", ["files": found])
        try save(root)
        warnings = answer["warnings"] as? [String] ?? []
        try refresh()
    }

    /// Every file below Music/, by its path there in composed Unicode, as the library writes it.
    private func music(_ dir: URL) -> [String: URL] {
        var out: [String: URL] = [:]
        let base = dir.standardizedFileURL.pathComponents.count
        let keys: [URLResourceKey] = [.isRegularFileKey]
        guard let walk = manager.enumerator(at: dir, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles]) else { return out }
        for case let url as URL in walk {
            guard (try? url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else { continue }
            let parts = url.standardizedFileURL.pathComponents.dropFirst(base)
            let path = parts.joined(separator: "/").precomposedStringWithCanonicalMapping
            out[path] = url
        }
        return out
    }

    /// Writes this phone's changed files, in the shared folder and in the app's own copy.
    func save(_ root: URL) throws {
        guard let unsaved = try Core.ask("unsaved") as? [[String: Any]], !unsaved.isEmpty else { return }
        let libraryDir = root.appendingPathComponent("Library", isDirectory: true)
        var written: [String] = []
        for file in unsaved {
            guard let name = file["name"] as? String, let text = file["text"] as? String, !blocked.contains(name) else { continue }
            try Data(text.utf8).write(to: backups().appendingPathComponent(name), options: .atomic)
            try write(text, to: libraryDir.appendingPathComponent(name))
            written.append(name)
        }
        try Core.ask("saved", ["names": written])
    }

    func refresh() throws {
        var lists = Lists()
        for s in try Core.ask("songs", ["sort": "artist"]) as? [[String: Any]] ?? [] {
            guard let id = s["id"] as? String else { continue }
            lists.songs[id] = Song(
                id: id,
                title: s["title"] as? String ?? "",
                artist: s["artist"] as? String ?? "",
                album: s["album"] as? String ?? "",
                durationMs: s["duration_ms"] as? Int ?? 0,
                loved: s["loved"] as? Bool ?? false,
                rating: s["rating"] as? Int ?? 0,
                here: s["here"] as? Bool ?? false,
                path: s["path"] as? String ?? ""
            )
            lists.order.append(id)
        }
        for a in try Core.ask("albums") as? [[String: Any]] ?? [] {
            lists.albums.append(Album(
                key: a["key"] as? String ?? UUID().uuidString,
                title: a["title"] as? String ?? "",
                artist: a["artist"] as? String ?? "",
                year: a["year"] as? Int ?? 0,
                songs: a["songs"] as? [String] ?? []
            ))
        }
        for p in try Core.ask("playlists") as? [[String: Any]] ?? [] {
            guard let id = p["id"] as? String else { continue }
            lists.playlists.append(PlaylistInfo(id: id, name: p["name"] as? String ?? "", smart: !(p["smart"] is NSNull) && p["smart"] != nil))
        }
        songs = lists
    }
}
