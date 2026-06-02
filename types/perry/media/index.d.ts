// Type declarations for perry/media — streaming media playback
// These types are auto-written by `perry init` / `perry types` so IDEs
// and tsc can resolve `import { ... } from "perry/media"`.

/**
 * Lifecycle state of a media player.
 *
 * - `idle` — created but not yet started loading
 * - `loading` — buffering / fetching headers
 * - `ready` — first chunk decoded, ready to play
 * - `playing` — actively rendering audio
 * - `paused` — playback paused (position preserved)
 * - `ended` — reached end of stream
 * - `error` — irrecoverable failure (network, codec, etc.)
 *
 * `ended` is fired both from the platform's end-of-playback signal
 * (`AVPlayerItemDidPlayToEndTimeNotification`, `MediaPlayer.OnCompletion`,
 * GStreamer `EOS`, MF `MEEnded`) **and** from a `currentTime ≈ duration`
 * fallback — see issue #351 for the rationale (Chromium / Chromecast have
 * historically dropped the native event in places, and the same belt-and-
 * braces is cheap to apply here).
 */
export type MediaState =
  | "idle"
  | "loading"
  | "ready"
  | "playing"
  | "paused"
  | "ended"
  | "error";

/**
 * Create a media player for the given URL. Returns a 1-based handle, or
 * `0` on failure. Supports HTTP/HTTPS streaming URLs (Subsonic, Icecast,
 * Shoutcast, plain progressive MP3/AAC, HLS m3u8) and `file://` paths.
 *
 * The player loads asynchronously. Register `onStateChange` first if you
 * want to call `play()` exactly when the player transitions to `"ready"`;
 * otherwise calling `play()` immediately also works — the platform backend
 * starts playback automatically once buffering completes.
 */
export function createPlayer(url: string): number;

/** Start (or resume) playback. */
export function play(handle: number): void;

/** Pause playback. The current position is preserved. */
export function pause(handle: number): void;

/** Stop playback and reset position to 0. */
export function stop(handle: number): void;

/** Seek to position in seconds. */
export function seek(handle: number, seconds: number): void;

/** Set volume on a 0.0–1.0 scale. Clamped at the boundary. */
export function setVolume(handle: number, volume: number): void;

/** Set playback rate (1.0 = normal speed). Apple platforms support 0.5–2.0. */
export function setRate(handle: number, rate: number): void;

/** Current playback position in seconds. */
export function getCurrentTime(handle: number): number;

/** Total duration in seconds. Returns 0 if unknown (live stream / still loading). */
export function getDuration(handle: number): number;

/** Current lifecycle state — see `MediaState`. */
export function getState(handle: number): MediaState;

/** Convenience boolean: `true` iff `getState(handle) === "playing"`. */
export function isPlaying(handle: number): boolean;

/**
 * Register a callback fired on every state transition. Replaces any
 * previously registered handler for this player.
 */
export function onStateChange(
  handle: number,
  callback: (state: MediaState) => void
): void;

/**
 * Register a callback fired ~10 times per second while playing, with the
 * current position and total duration in seconds. Replaces any previously
 * registered handler.
 */
export function onTimeUpdate(
  handle: number,
  callback: (current: number, duration: number) => void
): void;

/**
 * Set lock-screen / Control Center / Siri Remote metadata for this player.
 * Pass `""` for any field you don't have. `artworkUrl` may be a `file://`
 * path to a local image or an `https://` URL — the platform backend caches
 * remote artwork before display.
 *
 * Apple: backed by `MPNowPlayingInfoCenter` + `MPRemoteCommandCenter`.
 * Android: backed by `MediaSessionCompat`.
 * Linux/GTK4: backed by MPRIS D-Bus.
 * Windows: backed by `SystemMediaTransportControls`.
 */
export function setNowPlaying(
  handle: number,
  title: string,
  artist: string,
  album: string,
  artworkUrl: string
): void;

/** Destroy the player and free all resources. The handle becomes invalid. */
export function destroy(handle: number): void;
