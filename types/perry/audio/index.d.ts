// Type declarations for perry/audio — low-latency, game-engine-style audio.
//
// Three concept types (all opaque 1-based number handles, NaN-boxed at the
// FFI layer; you never construct them yourself):
//
//   - `Sound`      — a loaded asset (PCM buffer for preload, decoder state
//                    for streaming). Created with `loadSound`, freed with
//                    `unload`.
//   - `PlaybackId` — one *live* voice; what `play()` returns. Multiple
//                    PlaybackIds can exist for the same Sound (overlapping
//                    plays). Becomes invalid after `stop()` or end-of-file.
//   - `Bus`        — a mixer group. Sounds route through a Bus, Buses route
//                    through their parent (default: master). Use to scale or
//                    mute whole categories ("music" / "sfx" / "voice") with
//                    one call.
//
// All three live in disjoint integer ranges so APIs that accept "any handle"
// (e.g. `setVolume`, `stop`) can disambiguate cheaply at runtime.
//
// Distinct from `perry/media` — that's the streaming-with-UI path (AVPlayer,
// lock screen, now-playing). Use perry/audio for SFX, music loops, voice
// prompts, and any UI feedback that needs <20ms latency or overlap.

/** Opaque handle to a loaded sound asset (PCM buffer or stream decoder). */
export type Sound = number;

/** Opaque handle to a single live playback ("voice") of a sound. */
export type PlaybackId = number;

/** Opaque handle to a mixer bus. */
export type Bus = number;

/**
 * Load a sound asset and return a handle. Returns immediately — decode
 * happens in the background; if you call `play()` before decode finishes,
 * playback starts as soon as the buffer is ready.
 *
 * Path is bundle-relative on native, document-relative on the web target.
 *
 * @param path       Bundle path (e.g. `"assets/click.wav"`).
 * @param bus        Optional bus to route playback through. `0` (default) =
 *                   master bus.
 * @param stream     `true` = stream from disk (use for music or files >2MB).
 *                   `false` (default) = preload entire file as PCM. The
 *                   default is "auto" on the runtime side: files smaller
 *                   than ~2MB / 10s preload, larger ones stream — pass
 *                   `true` / `false` to override.
 *
 * Supported formats: **WAV and MP3 are portable across every platform**.
 * OGG Vorbis, FLAC and AAC are best-effort and depend on the platform
 * decoder — see `docs/api/audio.md` for the compatibility matrix.
 */
export function loadSound(
  path: string,
  bus?: Bus,
  stream?: boolean,
): Sound;

/** Free a sound's PCM buffer / stream decoder. The handle becomes invalid. */
export function unload(sound: Sound): void;

/**
 * Start a new playback voice. Returns a `PlaybackId` you can pass to
 * `stop`/`pause`/`resume`/`setVolume`/`setRate`/`setPan`. Calling `play()`
 * multiple times on the same `Sound` plays it concurrently (overlapping
 * voices); the same `Sound` buffer is shared — only the voice is new.
 *
 * @param sound      Handle returned by `loadSound`.
 * @param volume     0.0–1.0. Default `1.0`.
 * @param loop_      `true` loops indefinitely. Default `false`.
 * @param rate       Playback rate / pitch. `1.0` = normal, `0.5` = half
 *                   speed (octave down), `2.0` = double speed (octave up).
 *                   Apple supports 0.5–2.0; miniaudio and Web wider.
 * @param pan        Stereo pan, `-1.0` = full left, `0.0` = center,
 *                   `1.0` = full right. Default `0.0`.
 * @param fadeInMs   Linear-volume fade-in duration in ms. Default `0` =
 *                   instant.
 *
 * Tip for games: randomise `rate` in a small range (e.g. `0.95 +
 * Math.random() * 0.1`) on repeated SFX to avoid the "machine-gun" effect.
 */
export function play(
  sound: Sound,
  volume?: number,
  loop_?: boolean,
  rate?: number,
  pan?: number,
  fadeInMs?: number,
): PlaybackId;

/**
 * Stop a voice or every voice of a sound, optionally with a fade-out.
 *
 * - If `handle` is a `PlaybackId`: stops that single voice.
 * - If `handle` is a `Sound`: stops every live voice of that sound.
 *
 * @param fadeOutMs  Linear fade-out before stop. Default `0` = instant.
 */
export function stop(handle: PlaybackId | Sound, fadeOutMs?: number): void;

/** Pause a voice. Position is preserved; `resume(id)` continues from there. */
export function pause(playback: PlaybackId): void;

/** Resume a paused voice. */
export function resume(playback: PlaybackId): void;

/**
 * Set the volume on any handle:
 *
 * - `PlaybackId` — per-voice gain (overrides the sound's default).
 * - `Sound`      — default gain for *future* `play()` calls of this sound;
 *                  does not retroactively change live voices.
 * - `Bus`        — bus gain (multiplies every voice routed through this bus).
 *
 * `fadeMs` ramps linearly to the target volume. Default `0` = instant.
 */
export function setVolume(
  handle: PlaybackId | Sound | Bus,
  volume: number,
  fadeMs?: number,
): void;

/** Set playback rate (pitch) on a live voice. See `play()` for ranges. */
export function setRate(playback: PlaybackId, rate: number): void;

/** Set stereo pan on a live voice. `-1` left ↔ `+1` right. */
export function setPan(playback: PlaybackId, pan: number): void;

/**
 * Linearly fade a voice's gain up to `toVolume` (default `1.0`) over
 * `durationMs` ms. Convenience wrapper over `setVolume(id, toVolume,
 * durationMs)`.
 */
export function fadeIn(
  playback: PlaybackId,
  durationMs: number,
  toVolume?: number,
): void;

/**
 * Linearly fade a voice's gain to zero and then stop it. Equivalent to
 * `stop(playback, durationMs)`.
 */
export function fadeOut(playback: PlaybackId, durationMs: number): void;

/**
 * Crossfade between two voices: `fromPlayback` fades out while
 * `toPlayback` fades in, both linearly over `durationMs` ms. Standard
 * "music transition" primitive.
 */
export function crossfade(
  fromPlayback: PlaybackId,
  toPlayback: PlaybackId,
  durationMs: number,
): void;

/**
 * Create a named mixer bus. Sounds loaded with `bus = thisHandle` route
 * through it. Buses form a tree rooted at the implicit master bus; pass
 * `parent` to nest. Setting volume / mute / solo on a parent affects every
 * descendant.
 *
 * Typical setup:
 *
 *   const sfx   = createBus("sfx");
 *   const music = createBus("music");
 *   const voice = createBus("voice");
 */
export function createBus(name: string, parent?: Bus): Bus;

/** Destroy a bus. Voices currently routed through it route to master. */
export function destroyBus(bus: Bus): void;

/** Mute / unmute every voice routed through `bus`. */
export function muteBus(bus: Bus, muted: boolean): void;

/**
 * Solo / un-solo a bus. While at least one bus is soloed, every non-soloed
 * sibling is silenced. Sums across calls — use sparingly outside debug.
 */
export function soloBus(bus: Bus, soloed: boolean): void;

/** Set the master (root-bus) volume. `fadeMs` ramps linearly. */
export function setMasterVolume(volume: number, fadeMs?: number): void;

/**
 * Suspend the entire audio graph (every voice + the engine). Call from
 * `onAppDidEnterBackground` / `visibilitychange` to silence audio when the
 * app is backgrounded without losing voice state. Pair with `resumeAll()`.
 */
export function suspend(): void;

/** Resume after `suspend()`. */
export function resumeAll(): void;

/**
 * - Sound handle: `true` iff at least one voice of this sound is currently
 *   playing.
 * - PlaybackId: `true` iff this specific voice is in the `playing` state.
 */
export function isPlaying(handle: PlaybackId | Sound): boolean;

/** Total duration in seconds. Returns `0` if the sound is still decoding. */
export function getDuration(sound: Sound): number;

/** Current position in seconds for a live voice. */
export function getPosition(playback: PlaybackId): number;

/**
 * Register a callback fired once when the voice stops naturally (reaches
 * end-of-file with `loop = false`). Not fired for manual `stop()` or
 * `fadeOut()`. The voice is recycled after the callback runs — don't keep
 * the `PlaybackId` afterwards.
 */
export function onEnded(playback: PlaybackId, callback: () => void): void;

/**
 * Register a callback fired once when decode completes for a sound. If the
 * sound is already loaded by the time you register, the callback fires on
 * the next microtask tick.
 */
export function onLoaded(sound: Sound, callback: () => void): void;
