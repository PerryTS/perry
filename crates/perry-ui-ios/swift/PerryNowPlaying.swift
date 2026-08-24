import Foundation
import Observation
@_weakLinked import NowPlaying

@_silgen_name("perry_ios_now_playing_command")
private func perryNowPlayingCommand(_ handle: Int64, _ command: Int32, _ value: Double)

private func decodeNowPlayingUTF8(_ bytes: UnsafePointer<UInt8>?, _ length: Int32) -> String {
    guard let bytes, length > 0 else { return "" }
    return String(decoding: UnsafeBufferPointer(start: bytes, count: Int(length)), as: UTF8.self)
}

@available(iOS 27.0, *)
@Observable
@MainActor
private final class PerryNowPlayingModel: MediaSessionRepresentable {
    let handle: Int64
    let id: String
    var title: String
    var artist: String
    var album: String
    var artworkURL: String
    var stateCode: Int32
    var elapsedTime: TimeInterval
    var duration: TimeInterval
    var timestamp: Date

    init(
        handle: Int64,
        title: String,
        artist: String,
        album: String,
        artworkURL: String,
        stateCode: Int32,
        elapsedTime: TimeInterval,
        duration: TimeInterval
    ) {
        self.handle = handle
        self.id = "perry-media-\(handle)"
        self.title = title
        self.artist = artist
        self.album = album
        self.artworkURL = artworkURL
        self.stateCode = stateCode
        self.elapsedTime = elapsedTime
        self.duration = duration
        self.timestamp = .now
    }

    var content: (any MediaContentRepresentable)? {
        let artwork: Artwork? = if artworkURL.isEmpty {
            nil
        } else {
            Artwork(id: artworkURL) { [artworkURL] _ in
                guard let url = URL(string: artworkURL) else {
                    throw URLError(.badURL)
                }
                return try ArtworkRepresentation(data: Data(contentsOf: url))
            }
        }
        return MusicContent(
            id: "\(id)-\(title)-\(album)",
            songTitle: title,
            artistName: artist,
            albumName: album,
            type: .audio,
            duration: duration > 0 ? .finite(duration) : nil,
            artwork: artwork
        )
    }

    var playbackSnapshot: MediaPlaybackSnapshot? {
        MediaPlaybackSnapshot(
            state: stateCode == 3 ? .playing(rate: 1.0) : .paused,
            elapsedTime: elapsedTime,
            timestamp: timestamp
        )
    }

    var commands: [MediaCommand] {
        [
            .play { perryNowPlayingCommand(self.handle, 1, 0) },
            .pause { perryNowPlayingCommand(self.handle, 2, 0) },
            .stop { perryNowPlayingCommand(self.handle, 3, 0) },
            .seekToPosition { value in
                perryNowPlayingCommand(self.handle, 4, value)
            },
        ]
    }

    func updateMetadata(title: String, artist: String, album: String, artworkURL: String) {
        self.title = title
        self.artist = artist
        self.album = album
        self.artworkURL = artworkURL
    }

    func updateSnapshot(stateCode: Int32, elapsedTime: TimeInterval, duration: TimeInterval) {
        self.stateCode = stateCode
        self.elapsedTime = elapsedTime
        self.duration = duration
        self.timestamp = .now
    }
}

@available(iOS 27.0, *)
@MainActor
private final class PerryNowPlayingSessions {
    static let shared = PerryNowPlayingSessions()

    private struct Entry {
        let model: PerryNowPlayingModel
        let session: MediaSession<PerryNowPlayingModel>
    }

    private var entries: [Int64: Entry] = [:]

    func publish(
        handle: Int64,
        title: String,
        artist: String,
        album: String,
        artworkURL: String,
        stateCode: Int32,
        elapsedTime: TimeInterval,
        duration: TimeInterval
    ) {
        if let entry = entries[handle] {
            entry.model.updateMetadata(
                title: title,
                artist: artist,
                album: album,
                artworkURL: artworkURL
            )
            entry.model.updateSnapshot(
                stateCode: stateCode,
                elapsedTime: elapsedTime,
                duration: duration
            )
            return
        }

        let model = PerryNowPlayingModel(
            handle: handle,
            title: title,
            artist: artist,
            album: album,
            artworkURL: artworkURL,
            stateCode: stateCode,
            elapsedTime: elapsedTime,
            duration: duration
        )
        let session = MediaSession(model)
        entries[handle] = Entry(model: model, session: session)
        Task {
            try? await session.requestToBecomeSystemPrimary()
        }
    }

    func update(handle: Int64, stateCode: Int32, elapsedTime: TimeInterval, duration: TimeInterval) {
        entries[handle]?.model.updateSnapshot(
            stateCode: stateCode,
            elapsedTime: elapsedTime,
            duration: duration
        )
    }

    func remove(handle: Int64) {
        entries.removeValue(forKey: handle)
    }
}

@_cdecl("perry_swift_now_playing_is_available")
public func perrySwiftNowPlayingIsAvailable() -> Int32 {
    if #available(iOS 27.0, *) {
        return 1
    }
    return 0
}

@_cdecl("perry_swift_now_playing_publish")
public func perrySwiftNowPlayingPublish(
    _ handle: Int64,
    _ titleBytes: UnsafePointer<UInt8>?,
    _ titleLength: Int32,
    _ artistBytes: UnsafePointer<UInt8>?,
    _ artistLength: Int32,
    _ albumBytes: UnsafePointer<UInt8>?,
    _ albumLength: Int32,
    _ artworkBytes: UnsafePointer<UInt8>?,
    _ artworkLength: Int32,
    _ stateCode: Int32,
    _ elapsedTime: Double,
    _ duration: Double
) {
    guard #available(iOS 27.0, *) else { return }
    let title = decodeNowPlayingUTF8(titleBytes, titleLength)
    let artist = decodeNowPlayingUTF8(artistBytes, artistLength)
    let album = decodeNowPlayingUTF8(albumBytes, albumLength)
    let artworkURL = decodeNowPlayingUTF8(artworkBytes, artworkLength)
    Task { @MainActor in
        PerryNowPlayingSessions.shared.publish(
            handle: handle,
            title: title,
            artist: artist,
            album: album,
            artworkURL: artworkURL,
            stateCode: stateCode,
            elapsedTime: elapsedTime,
            duration: duration
        )
    }
}

@_cdecl("perry_swift_now_playing_update")
public func perrySwiftNowPlayingUpdate(
    _ handle: Int64,
    _ stateCode: Int32,
    _ elapsedTime: Double,
    _ duration: Double
) {
    guard #available(iOS 27.0, *) else { return }
    Task { @MainActor in
        PerryNowPlayingSessions.shared.update(
            handle: handle,
            stateCode: stateCode,
            elapsedTime: elapsedTime,
            duration: duration
        )
    }
}

@_cdecl("perry_swift_now_playing_remove")
public func perrySwiftNowPlayingRemove(_ handle: Int64) {
    guard #available(iOS 27.0, *) else { return }
    Task { @MainActor in
        PerryNowPlayingSessions.shared.remove(handle: handle)
    }
}
