# Media Playback

The `perry/media` module provides streaming media playback — HTTP/HTTPS
audio URLs (Subsonic, Icecast, plain MP3/AAC, HLS m3u8), `file://` paths,
lock-screen / Now Playing metadata, and remote-command (Siri Remote /
Touch Bar / Control Center) integration.

## Quick start

```typescript,no-test
import {
  createPlayer,
  play,
  pause,
  setVolume,
  onStateChange,
  onTimeUpdate,
  setNowPlaying,
} from "perry/media";

const player = createPlayer("https://example.com/track.mp3");
if (player === 0) {
  console.error("createPlayer failed");
} else {
  setVolume(player, 0.8);
  setNowPlaying(player, "Track Title", "Artist", "Album", "");

  onStateChange(player, (state) => console.log("state:", state));
  onTimeUpdate(player, (cur, dur) => console.log(`${cur}/${dur}s`));

  play(player); // begins (or resumes) once buffered
}
```

## API surface

| Function | Returns | Notes |
| --- | --- | --- |
| `createPlayer(url)` | handle (1+) or `0` on failure | HTTP/HTTPS or `file://` |
| `play(handle)` | void | Resumes if paused |
| `pause(handle)` | void | Position preserved |
| `stop(handle)` | void | Resets position to 0 |
| `seek(handle, seconds)` | void | |
| `setVolume(handle, volume)` | void | 0.0–1.0, clamped |
| `setRate(handle, rate)` | void | 1.0 = normal; Apple supports 0.5–2.0 |
| `getCurrentTime(handle)` | seconds | |
| `getDuration(handle)` | seconds | `0` if live / loading |
| `getState(handle)` | `MediaState` | See states below |
| `isPlaying(handle)` | boolean | |
| `onStateChange(h, cb)` | void | Fires on every transition |
| `onTimeUpdate(h, cb)` | void | ~10 Hz while playing |
| `setNowPlaying(h, title, artist, album, artworkUrl)` | void | All strings; pass `""` for unknown |
| `destroy(handle)` | void | Frees resources |

## States

`MediaState` is one of:

- `idle` — never started
- `loading` — buffering / fetching headers
- `ready` — first chunk decoded, ready to `play()`
- `playing` — actively rendering
- `paused` — paused (position preserved)
- `ended` — reached end of stream
- `error` — irrecoverable failure (network, codec, …)

### `ended` reliability

`ended` is fired both from the platform's native end-of-playback signal
**and** from a `currentTime ≈ duration` fallback. Per [issue #351
discussion](https://github.com/PerryTS/perry/issues/351), the native
event has been historically flaky on the web / Chromecast — the same
belt-and-braces is cheap to apply on every backend so a `perry/media`
consumer can rely on `ended` firing once per track.

The fallback engages only after `play()` has been called and `duration`
is known (live streams report `+inf`, which sanitises to `0` and disables
the fallback). Window: 0.25s before duration. The native signal sets the
flag first when it works; the fallback sets the same flag on the polling
tick if the signal hasn't arrived.

## Platform implementations

| Platform | Backend | Status |
| --- | --- | --- |
| macOS | AVPlayer + MPNowPlayingInfoCenter + MPRemoteCommandCenter | **Implemented** + lock-screen |
| iOS | AVPlayer + AVAudioSession Playback + UIImage artwork | **Implemented** + lock-screen |
| tvOS | AVPlayer + Siri Remote play/pause/skip | **Implemented** + remote |
| visionOS | AVPlayer + UIImage artwork | **Implemented** + lock-screen |
| Android | `android.media.MediaPlayer` + `MediaSessionCompat` via JNI | **Implemented** + lock-screen |
| GTK4 / Linux | GStreamer `playbin` element + MPRIS D-Bus | **Implemented** + lock-screen |
| Windows | `Windows.Media.Playback.MediaPlayer` (WinRT) + `SystemMediaTransportControls` | **Implemented** + Now Playing |
| watchOS | AVPlayer + AVAudioSession Playback + UIImage artwork | **Implemented** + Now Playing complication |
| HarmonyOS | `@ohos.multimedia.media.AVPlayer` via napi | **Implemented** (lock-screen via `@ohos.multimedia.avsession` is a follow-up) |
| Web | `<audio>` element + Media Session API | **Implemented** (`--target web`; `setNowPlaying` populates `navigator.mediaSession.metadata` + wires play / pause / seekto / seekforward / seekbackward action handlers) |

Stub platforms link cleanly against the same FFI surface — code that
imports `perry/media` compiles on every target. `createPlayer` returns
`0` on a stub backend so `if (player === 0)` is the canonical "feature
not available here" check.

On Linux, `setNowPlaying` exposes the player to the desktop via MPRIS
(`org.mpris.MediaPlayer2.perry-<pid>` on the session bus). GNOME Shell,
KDE Plasma, `playerctl`, and any Bluetooth-headphone media-key bridge
that speaks MPRIS will see the metadata and route Play / Pause /
PlayPause / Stop / Seek / SetPosition back to the player. The MPRIS
server is lazy-bootstrapped on the first `setNowPlaying` call so apps
that don't need lock-screen integration don't pay the zbus startup
cost. `Next` / `Previous` are no-ops (single-track playback model);
playlists are an app-level concern.

### Android — background playback

Perry's Android backend wires `MediaSessionCompat` so the lock-screen
tile, Bluetooth headset, Android Auto, and Wear OS see the metadata
pushed by `setNowPlaying` and route headphone play/pause/stop/seek
events back into the registered `onStateChange` closure. That covers
foreground use. Apps that want playback to survive the activity being
backgrounded (a podcast app, music player, etc.) need a foreground
service of their own — Android will otherwise kill the audio when the
process drops to the cached state. Add the following to your app's
`AndroidManifest.xml` and start the service when playback begins:

```xml
<service
    android:name=".PerryMediaService"
    android:foregroundServiceType="mediaPlayback"
    android:exported="false" />
<uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
<uses-permission android:name="android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK" />
```

The service implementation is app-specific — it should hold a
`MediaSessionCompat.Token` (the same session Perry created), build a
`Notification.MediaStyle` notification from it, and call
`startForeground(...)` on `play` / `stopForeground(false)` on `pause` /
`stopSelf()` on `stop`. We deliberately don't ship a default service
because the notification's branding (small icon, tint, content intent)
depends on the host app.

### Threading notes

The `onStateChange` and `onTimeUpdate` callbacks fire from the platform's
main UI thread on every backend, so they share the same JS heap as the
calling code. Implementation detail varies:

- **macOS / iOS / tvOS / visionOS** — driven by an `NSTimer` scheduled
  on the main run loop at 10 Hz.
- **Android** — driven from `Java_com_perry_app_PerryBridge_nativePumpTick`
  (the existing 125 Hz UI-thread pump), throttled internally to ~10 Hz.
  The `prepare()` call runs on a background worker thread to avoid
  blocking the UI on network buffering.
- **GTK4** — driven by a `glib::timeout_add_local` timer on the GLib
  main loop. EOS / error messages arrive on the GStreamer bus and get
  forwarded to per-player atomic flags via a `bus.add_watch_local`
  closure.
- **Windows** — driven from the `GetMessageW` / `PeekMessageW` message
  loop after each dispatch, throttled to 100 ms by wall-clock comparison.
- **HarmonyOS** — Perry's `.so` cannot reach `@ohos.multimedia.media`
  directly, so `perry/media` calls record intents into Mutex-protected
  drain queues in `perry-runtime::media_playback`. The harvested
  `pages/Index.ets` (emitted by `perry-codegen-arkts` whenever the
  module uses `perry/media`) installs a 100 ms `setInterval` pump in
  `aboutToAppear` that drains the queues, dispatches each op against
  the matching `media.AVPlayer` instance (allocated lazily on the
  first `createPlayer` drain), and pushes state observations back into
  the runtime via the `pushMediaState(handle, state, current,
  duration)` NAPI export. AVPlayer's own `stateChange` / `timeUpdate`
  / `error` / `endOfStream` events feed the same callback path. The
  pump runs on the ArkTS UI thread, so closures fired by
  `media_playback::push_media_state` share the same arena as Perry's
  `main()`. Lock-screen integration (`@ohos.multimedia.avsession`) is
  a follow-up — the runtime queues now-playing metadata via
  `drainNowPlaying` but the ArkTS-side AVSession dispatch is a no-op
  beyond a hilog line for now (tracked under issue #369).

## Now Playing on Apple platforms

Apple's MPNowPlayingInfoCenter is a process-wide singleton — the most
recent `setNowPlaying` call wins. For a single-player app (Subsonic
client, podcast player) this matches user expectation. The
MPRemoteCommandCenter handlers route `play` / `pause` / `togglePlayPause`
events to the **first live player handle** — multi-player apps that
need an explicit "active player" should manage that themselves.

`artworkUrl` accepts:

- `file://` paths — loaded synchronously via NSImage / UIImage
- `https://` URLs — fetched synchronously via NSData(contentsOf:) and
  wrapped in UIImage. The synchronous fetch is acceptable for a one-off
  artwork load (the MPNowPlayingInfoCenter dict is consumed
  synchronously when set).

### watchOS Info.plist requirements

watchOS keeps the audio engine alive when the watch screen sleeps **only
if** the app's `Info.plist` declares the `audio` background mode under
`WKBackgroundModes` (the WatchKit equivalent of iOS's `UIBackgroundModes`):

```xml
<key>WKBackgroundModes</key>
<array>
    <string>audio</string>
</array>
```

Without this entry the OS suspends the watch app a few seconds after the
wrist-down gesture or screen timeout, regardless of whether AVPlayer is
actively rendering. The runtime also auto-activates an `AVAudioSession`
with category `Playback` on the first `createPlayer(...)` call — combined
with the Info.plist entry, this is what tells watchOS the app intends to
keep playing audio in the background.

The Now Playing surface on the watch face is independent from the paired
iPhone's lock screen — they're separate processes with separate
`MPNowPlayingInfoCenter` instances. `setNowPlaying` on watchOS targets
the watch's Now Playing complication / glance screen.

## Subsonic example

```typescript,no-test
import { createPlayer, play, setNowPlaying, onStateChange } from "perry/media";

function streamUrl(serverUrl: string, user: string, pass: string, songId: string): string {
  const params = new URLSearchParams({
    u: user, p: pass, v: "1.16.1", c: "PerryClient", id: songId, format: "mp3",
  });
  return `${serverUrl}/rest/stream?${params.toString()}`;
}

const player = createPlayer(streamUrl("https://music.example.com", "alice", "secret", "12345"));
setNowPlaying(player, "All These Things That I've Done", "The Killers", "Hot Fuss",
              "https://music.example.com/rest/getCoverArt?id=12345&u=alice&p=secret&v=1.16.1&c=PerryClient");
onStateChange(player, (state) => {
  if (state === "ended") {
    // queue.next() ...
  }
});
play(player);
```

## Next steps

- [Audio Capture](audio.md) — Microphone input + dB metering
- [Overview](overview.md) — All system APIs
