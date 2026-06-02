//! perry/audio backend on Windows (#1867).
//!
//! The implementation lives in the shared `perry-audio-miniaudio` crate
//! so Linux / Windows / Android all share one backend instead of
//! triplicated stubs. miniaudio (single-header C, MIT) picks the native
//! audio API at runtime on Windows: WASAPI → DirectSound → WinMM.
//!
//! All 24 dispatch-table symbols (see
//! `crates/perry-dispatch/src/audio_table.rs`) are exported by
//! `perry-audio-miniaudio` as `#[no_mangle] extern "C"`. The thin
//! re-export wrappers below force the linker to keep them in
//! `libperry_ui_windows.a` — Rust's dead-code elimination would
//! otherwise drop transitive `no_mangle` symbols that nothing inside
//! the crate references.

extern crate perry_audio_miniaudio;

macro_rules! reexport {
    (fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        #[no_mangle]
        pub extern "C" fn $name($($arg: $ty),*) -> $ret {
            ::perry_audio_miniaudio::$name($($arg),*)
        }
    };
    (fn $name:ident($($arg:ident: $ty:ty),*)) => {
        #[no_mangle]
        pub extern "C" fn $name($($arg: $ty),*) {
            ::perry_audio_miniaudio::$name($($arg),*)
        }
    };
}

reexport!(fn perry_audio_load_sound(path_ptr: i64, bus: f64, stream: f64) -> i64);
reexport!(fn perry_audio_unload(sound: f64));
reexport!(fn perry_audio_play(sound: f64, volume: f64, loop_: f64, rate: f64, pan: f64, fade_in_ms: f64) -> i64);
reexport!(fn perry_audio_stop(handle: f64, fade_out_ms: f64));
reexport!(fn perry_audio_pause(playback: f64));
reexport!(fn perry_audio_resume(playback: f64));
reexport!(fn perry_audio_set_volume(handle: f64, volume: f64, fade_ms: f64));
reexport!(fn perry_audio_set_rate(playback: f64, rate: f64));
reexport!(fn perry_audio_set_pan(playback: f64, pan: f64));
reexport!(fn perry_audio_fade_in(playback: f64, duration_ms: f64, to_vol: f64));
reexport!(fn perry_audio_fade_out(playback: f64, duration_ms: f64));
reexport!(fn perry_audio_crossfade(from: f64, to: f64, duration_ms: f64));
reexport!(fn perry_audio_create_bus(name_ptr: i64, parent: f64) -> i64);
reexport!(fn perry_audio_destroy_bus(bus: f64));
reexport!(fn perry_audio_mute_bus(bus: f64, muted: f64));
reexport!(fn perry_audio_solo_bus(bus: f64, soloed: f64));
reexport!(fn perry_audio_set_master_volume(volume: f64, fade_ms: f64));
reexport!(fn perry_audio_suspend());
reexport!(fn perry_audio_resume_all());
reexport!(fn perry_audio_is_playing(handle: f64) -> f64);
reexport!(fn perry_audio_get_duration(sound: f64) -> f64);
reexport!(fn perry_audio_get_position(playback: f64) -> f64);
reexport!(fn perry_audio_on_ended(playback: f64, callback: f64));
reexport!(fn perry_audio_on_loaded(sound: f64, callback: f64));
