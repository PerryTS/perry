# Audio (perry/audio)

The `perry/audio` module is Perry's **low-latency, game-engine-style audio
mixer**. Three concepts:

- **`Sound`** — a loaded asset. `loadSound("click.wav")` returns one
  handle; the PCM data lives in memory until you `unload()`.
- **`PlaybackId`** — one *live voice*. `play(sound)` returns a new
  PlaybackId every time it's called, so the same sound can overlap with
  itself (think: multiple gunshots, multiple footsteps).
- **`Bus`** — a mixer group. Sounds route through a Bus, Buses route
  through their parent (default: master). One `setVolume(musicBus,
  0.3)` scales every voice on it.

Use `perry/audio` for SFX, music loops, voice prompts, and any UI
feedback where you want overlap or sub-20ms latency. For long-form
streaming with a seek bar, lock-screen controls, and Now Playing
metadata, use [`perry/media`](./media.md) instead.

## Quick start

```typescript,no-test
import {
  loadSound, play, stop, setVolume,
  createBus, setMasterVolume,
} from "perry/audio";

// Optional: organise sounds into buses
const sfx   = createBus("sfx");
const music = createBus("music");

// Load assets — decode happens in the background. The handle is
// returned immediately; play() before decode finishes just queues
// the playback.
const click = loadSound("assets/click.wav", sfx);
const bgm   = loadSound("assets/bgm.mp3",   music, /* stream */ true);

// Fire-and-forget — overlap is automatic, each play() returns a new
// PlaybackId you can stop / fade / tune independently.
const a = play(click);
const b = play(click, 0.7, false, 0.95);  // slightly lower pitch
const bgmId = play(bgm, 1.0, true);        // looping

// Mix
setVolume(music, 0.3);
setMasterVolume(0.8);

// Stop
stop(a);          // one voice
stop(click);      // every live voice of this sound
```

## Game-engine patterns

### Pitch variation on repeated SFX

The single biggest "doesn't feel robotic" trick: randomise the rate
(±5%) on every play of high-frequency SFX (footsteps, gunshots, hits).

```typescript,no-test
const rate = 0.95 + Math.random() * 0.1;  // 0.95 – 1.05
play(footstep, 1.0, false, rate);
```

### Crossfade music tracks

```typescript,no-test
const calmId = play(calm, 0.0, true);   // start silent
crossfade(intenseId, calmId, 2000);     // 2s linear crossfade
```

### Pause when backgrounded

```typescript,no-test
// from your app lifecycle hook (perry/system / onAppDidEnterBackground)
suspend();                              // silences everything
// onAppDidBecomeActive:
resumeAll();
```

### Three-bus mix template

```typescript,no-test
const sfx   = createBus("sfx");
const music = createBus("music");
const voice = createBus("voice");

// User-facing sliders bind to these:
setVolume(sfx,   userPreferences.sfxVolume);
setVolume(music, userPreferences.musicVolume);
setVolume(voice, userPreferences.voiceVolume);
```

## Format compatibility

WAV (PCM) and MP3 are **portable across every platform**. The rest depend
on the platform decoder:

| Format     | macOS / iOS / tvOS / visionOS | Linux / Windows / Android | Web |
|------------|:--:|:--:|:--:|
| WAV        | ✓ | ✓ | ✓ |
| MP3        | ✓ | ✓ | ✓ |
| AAC / M4A  | ✓ | ✗ | ✓ |
| OGG Vorbis | ✗ | ✓ | ✓ (most browsers) |
| FLAC       | ✓ (10.13+) | ✓ | partial (no Safari) |
| Opus       | ✓ (iOS 11+) | ✓ | ✓ |

When in doubt, ship **WAV for SFX** (small, instant decode) and **MP3
for music** (good compression, universal).

## Performance notes

- **Preload, decode once.** `loadSound` decodes a file to a single shared
  PCM buffer. Every subsequent `play()` of that sound schedules the same
  buffer — no re-decode, no second allocation. 1MB WAV = 1MB in RAM no
  matter how many times you play it.
- **Voice pool.** Voices are preallocated and recycled. The hot path
  through `play()` is one indexed table read plus a `scheduleBuffer`
  call. No malloc, no string lookup.
- **One shared audio graph.** A single `AVAudioEngine` (Apple) /
  `AudioContext` (Web) drives every sound. Bus volume / mute / solo are
  O(1) on a mixer node, not a walk over voices.
- **Streaming for big files only.** Pass `stream: true` to `loadSound`
  for music or files >2MB — Perry reads chunks from disk as the voice
  consumes them, so a 60-minute track doesn't occupy 60MB of RAM.
- **Target latency.** <10ms on Apple, <30ms on Web. On par with Unity /
  Godot.

## Platform implementation

| Platform | Backend |
|---|---|
| macOS / iOS / tvOS / visionOS | `AVAudioEngine` + `AVAudioPlayerNode` + `AVAudioPCMBuffer` + `AVAudioUnitVarispeed` (per-voice rate). |
| watchOS                       | Same `AVAudioEngine` stack as iOS. Background audio requires the host app to declare the audio background mode entitlement; foreground playback works out of the box. |
| Web (WASM)                    | Web Audio API (`AudioContext` + `AudioBufferSourceNode` + `GainNode`) |
| Linux / Windows / Android     | miniaudio v0.11.22 (`perry-audio-miniaudio` crate). PulseAudio / PipeWire / ALSA on Linux, WASAPI / DirectSound / WinMM on Windows, AAudio (API 26+) / OpenSL ES on Android — chosen at runtime. |

## Web autoplay policy

Browsers don't allow audio playback before a user gesture. The
`AudioContext` is lazily created on the first `loadSound()` / `play()`
call; if that call happens **before** any user interaction, the context
starts in a suspended state and your `play()` is queued. Trigger a
user-interaction-bound `resumeAll()` (or just any other `play()`
inside a click handler) to release it.

## API reference

See [the TypeScript declarations](../../../types/perry/audio/index.d.ts)
for full parameter documentation. Summary:

| Function | Purpose |
|---|---|
| `loadSound(path, bus?, stream?) -> Sound` | Decode (or open for streaming) an audio file. |
| `unload(sound)` | Free the PCM buffer / stream decoder. |
| `play(sound, volume?, loop?, rate?, pan?, fadeInMs?) -> PlaybackId` | Start a new voice. |
| `stop(handle, fadeOutMs?)` | Stop one voice or every voice of a sound. |
| `pause(playback)` / `resume(playback)` | Pause/resume a single voice. |
| `setVolume(handle, volume, fadeMs?)` | Sound default / live voice / bus. |
| `setRate(playback, rate)` / `setPan(playback, pan)` | Per-voice pitch and stereo position. |
| `fadeIn(playback, ms, toVol?)` / `fadeOut(playback, ms)` / `crossfade(a, b, ms)` | Linear ramps. |
| `createBus(name, parent?) -> Bus` / `destroyBus(bus)` / `muteBus(bus, muted)` / `soloBus(bus, soloed)` | Mixer tree. |
| `setMasterVolume(volume, fadeMs?)` | Root-bus gain. |
| `suspend()` / `resumeAll()` | Whole-graph pause for foreground/background transitions. |
| `isPlaying(handle)` / `getDuration(sound)` / `getPosition(playback)` | Introspection. |
| `onEnded(playback, cb)` / `onLoaded(sound, cb)` | Lifecycle callbacks. |

Tracked in issue [#1867](https://github.com/PerryTS/perry/issues/1867).
