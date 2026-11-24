import AVFoundation
import Foundation
import MediaPlayer

/// Plays the songs that are on this phone, in the background too, with the lock screen's and
/// the headphones' controls. Songs played to the end and songs skipped early are counted.
@MainActor
final class Player: ObservableObject {
    @Published private(set) var current: Song?
    @Published private(set) var playing = false

    weak var library: Library?
    private let player = AVPlayer()
    private var queue: [Song] = []
    private var index = 0
    private var ended: NSObjectProtocol?

    init() {
        ended = NotificationCenter.default.addObserver(
            forName: .AVPlayerItemDidPlayToEndTime, object: nil, queue: .main
        ) { [weak self] note in
            Task { @MainActor in self?.finished(note.object as? AVPlayerItem) }
        }
        let center = MPRemoteCommandCenter.shared()
        center.playCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.resume() }
            return .success
        }
        center.pauseCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.pause() }
            return .success
        }
        center.togglePlayPauseCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.toggle() }
            return .success
        }
        center.nextTrackCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.next() }
            return .success
        }
        center.previousTrackCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.previous() }
            return .success
        }
    }

    /// Plays `ids` from `start`, the songs that are on this phone.
    func play(_ ids: [String], from start: Int) {
        guard let library else { return }
        var songs: [Song] = []
        var first = 0
        for (i, id) in ids.enumerated() {
            guard let song = library.songs[id], song.here, library.fileURL(song) != nil else { continue }
            if i == start { first = songs.count }
            songs.append(song)
        }
        guard !songs.isEmpty else { return }
        queue = songs
        index = first
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .default)
        try? AVAudioSession.sharedInstance().setActive(true)
        load()
    }

    func toggle() { playing ? pause() : resume() }

    func resume() {
        guard current != nil else { return }
        player.play()
        playing = true
        show()
    }

    func pause() {
        player.pause()
        playing = false
        show()
    }

    func next() {
        guard let song = current else { return }
        let at = player.currentTime().seconds * 1000
        if song.durationMs > 0, at.isFinite, at < Double(song.durationMs) / 2 { library?.skipped(song.id) }
        advance(by: 1)
    }

    func previous() {
        if player.currentTime().seconds > 3 {
            player.seek(to: .zero)
        } else {
            advance(by: -1)
        }
    }

    private func finished(_ item: AVPlayerItem?) {
        guard item === player.currentItem, let song = current else { return }
        library?.played(song.id)
        advance(by: 1)
    }

    private func advance(by step: Int) {
        let to = index + step
        guard queue.indices.contains(to) else {
            pause()
            player.seek(to: .zero)
            return
        }
        index = to
        load()
    }

    private func load() {
        guard queue.indices.contains(index), let url = library?.fileURL(queue[index]) else { return }
        current = queue[index]
        player.replaceCurrentItem(with: AVPlayerItem(url: url))
        player.play()
        playing = true
        show()
    }

    /// What the lock screen and the control centre show.
    private func show() {
        guard let song = current else {
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            return
        }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = [
            MPMediaItemPropertyTitle: song.title,
            MPMediaItemPropertyArtist: song.artist,
            MPMediaItemPropertyAlbumTitle: song.album,
            MPMediaItemPropertyPlaybackDuration: Double(song.durationMs) / 1000,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: player.currentTime().seconds.isFinite ? player.currentTime().seconds : 0,
            MPNowPlayingInfoPropertyPlaybackRate: playing ? 1.0 : 0.0,
        ]
    }
}
