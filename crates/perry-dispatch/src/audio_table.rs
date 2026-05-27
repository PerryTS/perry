//! `PERRY_AUDIO_TABLE` — perry/audio low-latency sound playback (issue #1867).
//!
//! Game-engine-style audio: preloaded PCM buffers, voice pool, bus hierarchy,
//! pitch / pan / fade / loop. Distinct from `perry/media` (which is the
//! streaming-with-UI path: AVPlayer + lock screen + buffering). Use:
//!
//!   - **perry/audio** — SFX, music loops, voice prompts, anything where you
//!     want overlap, low latency, or a mixer with buses. Backed by
//!     `AVAudioEngine` on Apple, Web Audio API on browsers, and (PR 2)
//!     miniaudio on Linux / Windows / Android.
//!   - **perry/media** — long-form network media with seek bar, now-playing,
//!     lock-screen controls. Backed by AVPlayer / MediaPlayer / GStreamer /
//!     Media Foundation / `<audio>`.
//!
//! Handles are 1-based, NaN-boxed as f64 with `I64AsF64` return kind. The
//! three kinds (Sound / PlaybackId / Bus) live in disjoint ID ranges so the
//! runtime can disambiguate when a method accepts any of them (see
//! `perry-ui-macos/src/audio_playback.rs`):
//!
//!   - 0x00000001..=0x0FFFFFFF — Sound (PCM buffer)
//!   - 0x10000001..=0x1FFFFFFF — PlaybackId (live voice)
//!   - 0x20000001..=0x2FFFFFFF — Bus (mixer group)
//!
//! Booleans cross the FFI as f64 (0.0 / 1.0); optional numeric args default
//! to the sentinel the runtime documents (typically 1.0 for volume / rate,
//! 0.0 for everything else). All time arguments are milliseconds.

use super::*;

pub static PERRY_AUDIO_TABLE: &[MethodRow] = &[
    // ─── Lifecycle ────────────────────────────────────────────────────
    MethodRow {
        // loadSound(path, busHandle, stream) → soundHandle
        // busHandle = 0 ⇒ master bus. stream = 1.0 forces streaming
        // decode (for files >2MB / >10s); 0.0 = preload PCM.
        method: "loadSound",
        runtime: "perry_audio_load_sound",
        args: &[ArgKind::Str, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "unload",
        runtime: "perry_audio_unload",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ─── Playback control ─────────────────────────────────────────────
    MethodRow {
        // play(sound, volume, loop, rate, pan, fadeInMs) → playbackId
        method: "play",
        runtime: "perry_audio_play",
        args: &[
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        // stop(handle, fadeOutMs) — handle may be Sound or PlaybackId
        method: "stop",
        runtime: "perry_audio_stop",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "pause",
        runtime: "perry_audio_pause",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "resume",
        runtime: "perry_audio_resume",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ─── Per-voice / per-sound tuning ─────────────────────────────────
    MethodRow {
        // setVolume(handle, volume, fadeMs) — handle = Sound | PlaybackId | Bus
        method: "setVolume",
        runtime: "perry_audio_set_volume",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setRate",
        runtime: "perry_audio_set_rate",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setPan",
        runtime: "perry_audio_set_pan",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ─── Fades & crossfade ────────────────────────────────────────────
    MethodRow {
        // fadeIn(playback, durationMs, toVolume)
        method: "fadeIn",
        runtime: "perry_audio_fade_in",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        // fadeOut(playback, durationMs)
        method: "fadeOut",
        runtime: "perry_audio_fade_out",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        // crossfade(fromPlayback, toPlayback, durationMs)
        method: "crossfade",
        runtime: "perry_audio_crossfade",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ─── Buses ────────────────────────────────────────────────────────
    MethodRow {
        // createBus(name, parentBus) → busHandle. parentBus=0 ⇒ master.
        method: "createBus",
        runtime: "perry_audio_create_bus",
        args: &[ArgKind::Str, ArgKind::F64],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "destroyBus",
        runtime: "perry_audio_destroy_bus",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        // muteBus(bus, muted)
        method: "muteBus",
        runtime: "perry_audio_mute_bus",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        // soloBus(bus, soloed) — soloing a bus mutes all sibling buses
        method: "soloBus",
        runtime: "perry_audio_solo_bus",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ─── Master / engine ──────────────────────────────────────────────
    MethodRow {
        method: "setMasterVolume",
        runtime: "perry_audio_set_master_volume",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        // suspend() — pauses the entire audio graph (foreground/background).
        method: "suspend",
        runtime: "perry_audio_suspend",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        // resumeAll() — name avoids collision with `resume(playback)`.
        method: "resumeAll",
        runtime: "perry_audio_resume_all",
        args: &[],
        ret: ReturnKind::Void,
    },
    // ─── Introspection ────────────────────────────────────────────────
    MethodRow {
        method: "isPlaying",
        runtime: "perry_audio_is_playing",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getDuration",
        runtime: "perry_audio_get_duration",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getPosition",
        runtime: "perry_audio_get_position",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    // ─── Callbacks ────────────────────────────────────────────────────
    MethodRow {
        // onEnded(playback, callback) — fires once when the voice stops
        // naturally (loop=false). Cleared when the voice is recycled.
        method: "onEnded",
        runtime: "perry_audio_on_ended",
        args: &[ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        // onLoaded(sound, callback) — fires once decode completes. If the
        // sound is already loaded, fires on the next microtask tick.
        method: "onLoaded",
        runtime: "perry_audio_on_loaded",
        args: &[ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
];
