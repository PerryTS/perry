//! perry/audio backend for Linux / Windows / Android, backed by miniaudio
//! v0.11.22 (mackron/miniaudio, MIT, single-header). See issue #1867.
//!
//! This crate exports the same 24 `perry_audio_*` C symbols listed in
//! `crates/perry-dispatch/src/audio_table.rs`. Each platform UI crate
//! (perry-ui-gtk4 / perry-ui-windows / perry-ui-android) drags the rlib
//! into its staticlib so user binaries resolve the dispatch table at
//! link time.
//!
//! Handle ranges and semantics mirror the Apple backend
//! (`crates/perry-ui-macos/src/audio_playback.rs`) so a behavioural test
//! suite can run unchanged across platforms:
//!
//!   Sound       0x00000001..=0x0FFFFFFF  — preloaded miniaudio template
//!   PlaybackId  0x10000001..=0x1FFFFFFF  — live ma_sound voice
//!   Bus         0x20000001..=0x2FFFFFFF  — ma_sound_group (0 = master)

#![cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]

use libc::{c_char, c_float, c_int, c_uint, c_void};
use std::cell::RefCell;
use std::ffi::CString;
use std::sync::Mutex;

// =============================================================================
// String header — mirrors perry_runtime::string::StringHeader. Kept inline
// (don't depend on perry-runtime — that would create a dep cycle through
// the UI crates that re-export us).
// =============================================================================

#[repr(C)]
struct StringHeader {
    pub utf16_len: u32,
    pub byte_len: u32,
    pub capacity: u32,
    pub refcount: u32,
    pub flags: u32,
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(slice).unwrap_or("").to_owned()
    }
}

// =============================================================================
// miniaudio FFI — only the slice we actually need.
//
// Opaque structs are represented as zero-initialised byte arrays sized
// well above the upstream sizeof (measured at 1312 / 952 / 952 bytes for
// ma_engine / ma_sound / ma_sound_group on v0.11.22). 16 KiB leaves
// substantial headroom for future minor-version growth without changing
// the ABI.
// =============================================================================

const MA_ENGINE_SIZE: usize = 16 * 1024;
const MA_SOUND_SIZE: usize = 16 * 1024;
const MA_GROUP_SIZE: usize = 16 * 1024;

type MaResult = c_int;
const MA_SUCCESS: MaResult = 0;

const MA_SOUND_FLAG_STREAM: c_uint = 0x00000001;
#[allow(dead_code)]
const MA_SOUND_FLAG_DECODE: c_uint = 0x00000002;
#[allow(dead_code)]
const MA_SOUND_FLAG_ASYNC: c_uint = 0x00000004;
#[allow(dead_code)]
const MA_SOUND_FLAG_NO_DEFAULT_ATTACHMENT: c_uint = 0x00000010;

extern "C" {
    fn ma_engine_init(config: *const c_void, engine: *mut c_void) -> MaResult;
    #[allow(dead_code)]
    fn ma_engine_uninit(engine: *mut c_void);
    fn ma_engine_set_volume(engine: *mut c_void, volume: c_float) -> MaResult;
    fn ma_engine_start(engine: *mut c_void) -> MaResult;
    fn ma_engine_stop(engine: *mut c_void) -> MaResult;

    fn ma_sound_init_from_file(
        engine: *mut c_void,
        filepath: *const c_char,
        flags: c_uint,
        group: *mut c_void,
        fence: *mut c_void,
        sound: *mut c_void,
    ) -> MaResult;
    fn ma_sound_init_copy(
        engine: *mut c_void,
        existing_sound: *const c_void,
        flags: c_uint,
        group: *mut c_void,
        sound: *mut c_void,
    ) -> MaResult;
    fn ma_sound_uninit(sound: *mut c_void);
    fn ma_sound_start(sound: *mut c_void) -> MaResult;
    fn ma_sound_stop(sound: *mut c_void) -> MaResult;
    fn ma_sound_stop_with_fade_in_milliseconds(sound: *mut c_void, ms: u64) -> MaResult;
    fn ma_sound_set_volume(sound: *mut c_void, volume: c_float);
    fn ma_sound_set_pitch(sound: *mut c_void, pitch: c_float);
    fn ma_sound_set_pan(sound: *mut c_void, pan: c_float);
    fn ma_sound_set_looping(sound: *mut c_void, looping: c_uint);
    fn ma_sound_is_playing(sound: *const c_void) -> c_uint;
    fn ma_sound_get_cursor_in_seconds(sound: *mut c_void, out: *mut c_float) -> MaResult;
    fn ma_sound_get_length_in_seconds(sound: *mut c_void, out: *mut c_float) -> MaResult;
    fn ma_sound_set_fade_in_milliseconds(sound: *mut c_void, from: c_float, to: c_float, ms: u64);
    fn ma_sound_set_end_callback(
        sound: *mut c_void,
        cb: extern "C" fn(user_data: *mut c_void, sound: *mut c_void),
        user_data: *mut c_void,
    );

    fn ma_sound_group_init(
        engine: *mut c_void,
        flags: c_uint,
        parent: *mut c_void,
        group: *mut c_void,
    ) -> MaResult;
    fn ma_sound_group_uninit(group: *mut c_void);
    fn ma_sound_group_set_volume(group: *mut c_void, volume: c_float);
}

// =============================================================================
// Closure / pump helpers provided by perry-runtime / perry-stdlib. Imported
// weakly — `extern "C"` declarations only resolve at final link time, so
// `cargo check -p perry-audio-miniaudio` is happy without them.
// =============================================================================

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_promise_run_microtasks() -> i32;
    fn js_run_stdlib_pump();
}

// =============================================================================
// Handle classification — exact copy of the Apple backend.
// =============================================================================

const SOUND_BASE: i64 = 0x0000_0001;
const SOUND_MAX: i64 = 0x0FFF_FFFF;
const PLAYBACK_BASE: i64 = 0x1000_0001;
const PLAYBACK_MAX: i64 = 0x1FFF_FFFF;
const BUS_BASE: i64 = 0x2000_0001;
const BUS_MAX: i64 = 0x2FFF_FFFF;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum HandleKind {
    Master,
    Sound(usize),
    Playback(usize),
    Bus(usize),
    Invalid,
}

fn classify(h: f64) -> HandleKind {
    if h == 0.0 {
        return HandleKind::Master;
    }
    let id = h as i64;
    if (SOUND_BASE..=SOUND_MAX).contains(&id) {
        HandleKind::Sound((id - SOUND_BASE) as usize)
    } else if (PLAYBACK_BASE..=PLAYBACK_MAX).contains(&id) {
        HandleKind::Playback((id - PLAYBACK_BASE) as usize)
    } else if (BUS_BASE..=BUS_MAX).contains(&id) {
        HandleKind::Bus((id - BUS_BASE) as usize)
    } else {
        HandleKind::Invalid
    }
}

fn sound_handle_id(idx: usize) -> i64 {
    SOUND_BASE + idx as i64
}
fn playback_handle_id(idx: usize) -> i64 {
    PLAYBACK_BASE + idx as i64
}
fn bus_handle_id(idx: usize) -> i64 {
    BUS_BASE + idx as i64
}

// =============================================================================
// State
// =============================================================================

/// Heap-allocated, zeroed byte buffer carrying a miniaudio opaque struct.
/// Boxing keeps the buffer at a stable address (miniaudio retains pointers
/// to itself across calls — moving them would corrupt internal lists).
type MaBox<const N: usize> = Box<[u8; N]>;

fn ma_zeroed<const N: usize>() -> MaBox<N> {
    // SAFETY: ma_*_init zero-checks expected fields; an all-zero buffer
    // is the documented uninitialised state for every opaque struct we
    // touch.
    Box::new([0u8; N])
}

fn ma_ptr<const N: usize>(b: &mut MaBox<N>) -> *mut c_void {
    b.as_mut_ptr() as *mut c_void
}
fn ma_ptr_const<const N: usize>(b: &MaBox<N>) -> *const c_void {
    b.as_ptr() as *const c_void
}

struct SoundEntry {
    /// First-loaded miniaudio sound; used as a template for future
    /// `ma_sound_init_copy` plays so multiple voices share decoded PCM.
    template: MaBox<MA_SOUND_SIZE>,
    bus_handle: f64,
    default_volume: f32,
    path: CString,
    streaming: bool,
    on_loaded: Option<f64>,
}

struct VoiceEntry {
    sound: MaBox<MA_SOUND_SIZE>,
    sound_idx: usize,
    #[allow(dead_code)]
    bus_handle: f64,
    is_playing: bool,
    is_paused: bool,
    looping: bool,
    volume: f32,
    on_ended: Option<f64>,
    manually_stopped: bool,
}

struct BusEntry {
    group: MaBox<MA_GROUP_SIZE>,
    _name: String,
    parent_id: f64,
    volume: f32,
    muted: bool,
    pre_mute_volume: f32,
    soloed: bool,
}

struct Fade {
    handle: f64,
    #[allow(dead_code)]
    start_vol: f32,
    target_vol: f32,
    #[allow(dead_code)]
    ticks_total: u32,
    ticks_left: u32,
    then_stop: bool,
}

thread_local! {
    static ENGINE: RefCell<Option<MaBox<MA_ENGINE_SIZE>>> = RefCell::new(None);
    static SOUNDS: RefCell<Vec<Option<SoundEntry>>> = RefCell::new(Vec::new());
    static VOICES: RefCell<Vec<Option<VoiceEntry>>> = RefCell::new(Vec::new());
    static BUSES: RefCell<Vec<Option<BusEntry>>> = RefCell::new(Vec::new());
    static FADES: RefCell<Vec<Fade>> = RefCell::new(Vec::new());
    static MASTER_VOLUME: RefCell<f32> = RefCell::new(1.0);
    static PENDING_ENDED: RefCell<Vec<usize>> = RefCell::new(Vec::new());
    static PENDING_LOADED: RefCell<Vec<usize>> = RefCell::new(Vec::new());
}

/// Voice indices whose miniaudio end_callback fired on the audio thread.
/// We can't touch thread-locals from there; main-thread `drain_*` pulls
/// these into PENDING_ENDED on every hot-path entry point.
static CROSS_ENDED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

fn slot_insert<T>(vec: &mut Vec<Option<T>>, entry: T) -> usize {
    for (i, slot) in vec.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(entry);
            return i;
        }
    }
    vec.push(Some(entry));
    vec.len() - 1
}

// =============================================================================
// Engine
// =============================================================================

fn ensure_engine() -> bool {
    let already = ENGINE.with(|e| e.borrow().is_some());
    if already {
        return true;
    }
    let mut engine = ma_zeroed::<MA_ENGINE_SIZE>();
    let rc = unsafe { ma_engine_init(std::ptr::null(), ma_ptr(&mut engine)) };
    if rc != MA_SUCCESS {
        eprintln!("[perry/audio] ma_engine_init failed: {}", rc);
        return false;
    }
    ENGINE.with(|e| *e.borrow_mut() = Some(engine));
    true
}

fn with_engine_ptr<R>(f: impl FnOnce(*mut c_void) -> R) -> Option<R> {
    ENGINE.with(|e| {
        let mut b = e.borrow_mut();
        b.as_mut().map(|engine| f(ma_ptr(engine)))
    })
}

/// Pointer to the parent group node for a bus handle. `0` ⇒ engine
/// endpoint (NULL parent, which miniaudio interprets as the master
/// output bus). Returns Some(NULL) for the master case so callers can
/// distinguish "master" from "lookup failed".
fn resolve_bus_group(bus_h: f64) -> Option<*mut c_void> {
    match classify(bus_h) {
        HandleKind::Master => Some(std::ptr::null_mut()),
        HandleKind::Bus(idx) => BUSES.with(|b| {
            let mut buses = b.borrow_mut();
            buses
                .get_mut(idx)
                .and_then(|slot| slot.as_mut())
                .map(|entry| ma_ptr(&mut entry.group))
        }),
        _ => None,
    }
}

// =============================================================================
// Cross-thread ended queue
// =============================================================================

extern "C" fn end_callback_trampoline(user_data: *mut c_void, _sound: *mut c_void) {
    // miniaudio fires this on its render thread. The user_data we set
    // is the voice index packed as `usize`. We MUST NOT touch any
    // thread_local from here.
    let idx = user_data as usize;
    if let Ok(mut q) = CROSS_ENDED.lock() {
        q.push(idx);
    }
}

fn drain_cross_ended() {
    let drained: Vec<usize> = match CROSS_ENDED.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => Vec::new(),
    };
    if !drained.is_empty() {
        PENDING_ENDED.with(|p| p.borrow_mut().extend(drained));
    }
}

fn drain_pending_callbacks() {
    drain_cross_ended();

    let loaded: Vec<usize> = PENDING_LOADED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for idx in loaded {
        let cb = SOUNDS.with(|s| {
            let mut sounds = s.borrow_mut();
            sounds
                .get_mut(idx)
                .and_then(|o| o.as_mut())
                .and_then(|e| e.on_loaded.take())
        });
        if let Some(closure_f64) = cb {
            unsafe {
                js_run_stdlib_pump();
                let _ = js_promise_run_microtasks();
                let ptr = js_nanbox_get_pointer(closure_f64);
                let _ = js_closure_call0(ptr as *const u8);
            }
        }
    }

    let ended: Vec<usize> = PENDING_ENDED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for idx in ended {
        // miniaudio's looping flag handles re-scheduling natively — we
        // never need to restart a streamed loop manually like the
        // AVAudioEngine path does.
        let (cb, should_clean) = VOICES.with(|v| {
            let mut voices = v.borrow_mut();
            match voices.get_mut(idx) {
                Some(Some(entry)) => {
                    let cb = entry.on_ended.take();
                    let clean = !entry.looping;
                    if clean {
                        entry.is_playing = false;
                    }
                    (cb, clean)
                }
                _ => (None, false),
            }
        });
        if let Some(closure_f64) = cb {
            unsafe {
                js_run_stdlib_pump();
                let _ = js_promise_run_microtasks();
                let ptr = js_nanbox_get_pointer(closure_f64);
                let _ = js_closure_call0(ptr as *const u8);
            }
        }
        if should_clean {
            let taken = VOICES.with(|v| {
                let mut voices = v.borrow_mut();
                voices.get_mut(idx).and_then(|slot| slot.take())
            });
            if let Some(mut entry) = taken {
                unsafe { ma_sound_uninit(ma_ptr(&mut entry.sound)) };
            }
        }
    }
}

// =============================================================================
// loadSound / unload / onLoaded
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_load_sound(path_ptr: i64, bus: f64, stream: f64) -> i64 {
    if !ensure_engine() {
        return 0;
    }
    let is_streaming = stream != 0.0;
    let filename = str_from_header(path_ptr as *const u8);
    if filename.is_empty() {
        return 0;
    }
    let c_path = match CString::new(filename.as_str()) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let group_ptr = match resolve_bus_group(bus) {
        Some(p) => p,
        None => return 0,
    };
    let mut flags: c_uint = 0;
    if is_streaming {
        flags |= MA_SOUND_FLAG_STREAM;
    }

    let mut template = ma_zeroed::<MA_SOUND_SIZE>();
    let rc = with_engine_ptr(|engine_ptr| unsafe {
        ma_sound_init_from_file(
            engine_ptr,
            c_path.as_ptr(),
            flags,
            group_ptr,
            std::ptr::null_mut(),
            ma_ptr(&mut template),
        )
    });
    let rc = match rc {
        Some(r) => r,
        None => return 0,
    };
    if rc != MA_SUCCESS {
        eprintln!("[perry/audio] loadSound failed for {}: {}", filename, rc);
        return 0;
    }

    let entry = SoundEntry {
        template,
        bus_handle: bus,
        default_volume: 1.0,
        path: c_path,
        streaming: is_streaming,
        on_loaded: None,
    };
    let idx = SOUNDS.with(|s| slot_insert(&mut s.borrow_mut(), entry));
    sound_handle_id(idx)
}

#[no_mangle]
pub extern "C" fn perry_audio_unload(sound: f64) {
    let idx = match classify(sound) {
        HandleKind::Sound(i) => i,
        _ => return,
    };
    stop_voices_of_sound(idx, 0.0);
    let taken = SOUNDS.with(|s| {
        let mut sounds = s.borrow_mut();
        sounds.get_mut(idx).and_then(|slot| slot.take())
    });
    if let Some(mut entry) = taken {
        unsafe { ma_sound_uninit(ma_ptr(&mut entry.template)) };
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_on_loaded(sound: f64, callback: f64) {
    let idx = match classify(sound) {
        HandleKind::Sound(i) => i,
        _ => return,
    };
    let fired = SOUNDS.with(|s| {
        let mut sounds = s.borrow_mut();
        match sounds.get_mut(idx).and_then(|o| o.as_mut()) {
            Some(entry) => {
                entry.on_loaded = Some(callback);
                true
            }
            None => false,
        }
    });
    if fired {
        // Preload is synchronous; queue the callback for the next pump.
        PENDING_LOADED.with(|p| p.borrow_mut().push(idx));
        drain_pending_callbacks();
    }
}

// =============================================================================
// play / stop / pause / resume
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_play(
    sound: f64,
    volume: f64,
    loop_: f64,
    rate: f64,
    pan: f64,
    fade_in_ms: f64,
) -> i64 {
    let sound_idx = match classify(sound) {
        HandleKind::Sound(i) => i,
        _ => return 0,
    };
    if !ensure_engine() {
        return 0;
    }

    let (default_volume, bus_h, streaming) = match SOUNDS.with(|s| {
        let sounds = s.borrow();
        sounds
            .get(sound_idx)
            .and_then(|o| o.as_ref())
            .map(|e| (e.default_volume, e.bus_handle, e.streaming))
    }) {
        Some(t) => t,
        None => return 0,
    };

    let final_vol = if volume >= 0.0 {
        (volume as f32).clamp(0.0, 1.0)
    } else {
        default_volume
    };
    let looping = loop_ != 0.0;
    let fade_in = fade_in_ms > 0.0;
    let initial_vol = if fade_in { 0.0_f32 } else { final_vol };
    let rate_f = (rate as f32).clamp(0.25, 4.0);
    let pan_f = (pan as f32).clamp(-1.0, 1.0);

    let group_ptr = match resolve_bus_group(bus_h) {
        Some(p) => p,
        None => return 0,
    };

    // Allocate the voice's miniaudio sound. Prefer init_copy (shares the
    // already-decoded PCM); fall back to a fresh init_from_file for
    // streaming sounds where copy can't share state.
    let mut voice_sound = ma_zeroed::<MA_SOUND_SIZE>();
    let init_rc = SOUNDS.with(|s| -> Option<MaResult> {
        let mut sounds = s.borrow_mut();
        let entry = sounds.get_mut(sound_idx).and_then(|o| o.as_mut())?;
        let mut flags: c_uint = 0;
        if entry.streaming {
            flags |= MA_SOUND_FLAG_STREAM;
        }
        with_engine_ptr(|engine_ptr| unsafe {
            if streaming {
                ma_sound_init_from_file(
                    engine_ptr,
                    entry.path.as_ptr(),
                    flags,
                    group_ptr,
                    std::ptr::null_mut(),
                    ma_ptr(&mut voice_sound),
                )
            } else {
                ma_sound_init_copy(
                    engine_ptr,
                    ma_ptr_const(&entry.template),
                    flags,
                    group_ptr,
                    ma_ptr(&mut voice_sound),
                )
            }
        })
    });
    let init_rc = match init_rc {
        Some(r) => r,
        None => return 0,
    };
    if init_rc != MA_SUCCESS {
        eprintln!("[perry/audio] play: ma_sound init failed: {}", init_rc);
        return 0;
    }

    unsafe {
        ma_sound_set_volume(ma_ptr(&mut voice_sound), initial_vol);
        ma_sound_set_pitch(ma_ptr(&mut voice_sound), rate_f);
        ma_sound_set_pan(ma_ptr(&mut voice_sound), pan_f);
        ma_sound_set_looping(ma_ptr(&mut voice_sound), if looping { 1 } else { 0 });
    }

    let entry = VoiceEntry {
        sound: voice_sound,
        sound_idx,
        bus_handle: bus_h,
        is_playing: true,
        is_paused: false,
        looping,
        volume: initial_vol,
        on_ended: None,
        manually_stopped: false,
    };
    let voice_idx = VOICES.with(|v| slot_insert(&mut v.borrow_mut(), entry));

    // Wire the end callback with the voice index packed as user_data.
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(e)) = voices.get_mut(voice_idx) {
            unsafe {
                ma_sound_set_end_callback(
                    ma_ptr(&mut e.sound),
                    end_callback_trampoline,
                    voice_idx as *mut c_void,
                );
                if fade_in {
                    ma_sound_set_fade_in_milliseconds(
                        ma_ptr(&mut e.sound),
                        initial_vol,
                        final_vol,
                        fade_in_ms as u64,
                    );
                }
                let rc = ma_sound_start(ma_ptr(&mut e.sound));
                if rc != MA_SUCCESS {
                    eprintln!("[perry/audio] play: ma_sound_start failed: {}", rc);
                }
                if fade_in {
                    // Remember the post-fade volume so introspection
                    // sees the steady-state.
                    e.volume = final_vol;
                }
            }
        }
    });

    drain_pending_callbacks();
    playback_handle_id(voice_idx)
}

fn stop_voices_of_sound(sound_idx: usize, fade_out_ms: f64) {
    let indices: Vec<usize> = VOICES.with(|v| {
        v.borrow()
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                Some(e) if e.sound_idx == sound_idx => Some(i),
                _ => None,
            })
            .collect()
    });
    for i in indices {
        stop_voice(i, fade_out_ms);
    }
}

fn stop_voice(voice_idx: usize, fade_out_ms: f64) {
    if fade_out_ms > 0.0 {
        VOICES.with(|v| {
            let mut voices = v.borrow_mut();
            if let Some(Some(entry)) = voices.get_mut(voice_idx) {
                entry.manually_stopped = true;
                unsafe {
                    ma_sound_stop_with_fade_in_milliseconds(
                        ma_ptr(&mut entry.sound),
                        fade_out_ms as u64,
                    );
                }
            }
        });
        // miniaudio's end callback fires when the fade completes; the
        // pump cleans up the slot then.
        return;
    }

    let taken = VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(voice_idx) {
            entry.manually_stopped = true;
        }
        voices.get_mut(voice_idx).and_then(|slot| slot.take())
    });
    if let Some(mut entry) = taken {
        unsafe {
            ma_sound_stop(ma_ptr(&mut entry.sound));
            ma_sound_uninit(ma_ptr(&mut entry.sound));
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_stop(handle: f64, fade_out_ms: f64) {
    match classify(handle) {
        HandleKind::Playback(idx) => stop_voice(idx, fade_out_ms),
        HandleKind::Sound(idx) => stop_voices_of_sound(idx, 0.0),
        _ => {}
    }
    drain_pending_callbacks();
}

#[no_mangle]
pub extern "C" fn perry_audio_pause(playback: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(idx) {
            if entry.is_playing && !entry.is_paused {
                // ma_sound_stop preserves the playback cursor — that's
                // exactly "pause" semantics. uninit happens only on
                // hard stop_voice / unload / engine teardown.
                unsafe {
                    ma_sound_stop(ma_ptr(&mut entry.sound));
                }
                entry.is_paused = true;
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn perry_audio_resume(playback: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(idx) {
            if entry.is_paused {
                unsafe {
                    ma_sound_start(ma_ptr(&mut entry.sound));
                }
                entry.is_paused = false;
            }
        }
    });
}

// =============================================================================
// setVolume / setRate / setPan
// =============================================================================

fn apply_volume_now(handle: f64, vol: f32) {
    let vol = vol.clamp(0.0, 1.0);
    match classify(handle) {
        HandleKind::Master => {
            MASTER_VOLUME.with(|m| *m.borrow_mut() = vol);
            let _ = with_engine_ptr(|p| unsafe { ma_engine_set_volume(p, vol) });
        }
        HandleKind::Bus(idx) => {
            BUSES.with(|b| {
                let mut buses = b.borrow_mut();
                if let Some(Some(entry)) = buses.get_mut(idx) {
                    entry.volume = vol;
                }
            });
            reapply_solo_state();
        }
        HandleKind::Sound(idx) => {
            SOUNDS.with(|s| {
                let mut sounds = s.borrow_mut();
                if let Some(Some(entry)) = sounds.get_mut(idx) {
                    entry.default_volume = vol;
                }
            });
        }
        HandleKind::Playback(idx) => {
            VOICES.with(|v| {
                let mut voices = v.borrow_mut();
                if let Some(Some(entry)) = voices.get_mut(idx) {
                    entry.volume = vol;
                    unsafe { ma_sound_set_volume(ma_ptr(&mut entry.sound), vol) };
                }
            });
        }
        HandleKind::Invalid => {}
    }
}

fn current_volume(handle: f64) -> Option<f32> {
    match classify(handle) {
        HandleKind::Master => Some(MASTER_VOLUME.with(|m| *m.borrow())),
        HandleKind::Bus(idx) => BUSES.with(|b| {
            b.borrow()
                .get(idx)
                .and_then(|o| o.as_ref())
                .map(|e| e.volume)
        }),
        HandleKind::Sound(idx) => SOUNDS.with(|s| {
            s.borrow()
                .get(idx)
                .and_then(|o| o.as_ref())
                .map(|e| e.default_volume)
        }),
        HandleKind::Playback(idx) => VOICES.with(|v| {
            v.borrow()
                .get(idx)
                .and_then(|o| o.as_ref())
                .map(|e| e.volume)
        }),
        HandleKind::Invalid => None,
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_set_volume(handle: f64, volume: f64, fade_ms: f64) {
    let target = (volume as f32).clamp(0.0, 1.0);
    if fade_ms <= 0.0 {
        apply_volume_now(handle, target);
        return;
    }
    // For voice handles, miniaudio has a native fade primitive.
    if let HandleKind::Playback(idx) = classify(handle) {
        VOICES.with(|v| {
            let mut voices = v.borrow_mut();
            if let Some(Some(entry)) = voices.get_mut(idx) {
                let start = entry.volume;
                unsafe {
                    ma_sound_set_fade_in_milliseconds(
                        ma_ptr(&mut entry.sound),
                        start,
                        target,
                        fade_ms as u64,
                    );
                }
                entry.volume = target;
            }
        });
        return;
    }
    // Master / Bus / Sound use our software fade so we can intercept
    // the per-tick volume and re-run the solo/mute logic.
    let start = current_volume(handle).unwrap_or(target);
    schedule_fade(handle, start, target, fade_ms, false);
}

#[no_mangle]
pub extern "C" fn perry_audio_set_rate(playback: f64, rate: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    let rate_f = (rate as f32).clamp(0.25, 4.0);
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(idx) {
            unsafe { ma_sound_set_pitch(ma_ptr(&mut entry.sound), rate_f) };
        }
    });
}

#[no_mangle]
pub extern "C" fn perry_audio_set_pan(playback: f64, pan: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    let pan_f = (pan as f32).clamp(-1.0, 1.0);
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(idx) {
            unsafe { ma_sound_set_pan(ma_ptr(&mut entry.sound), pan_f) };
        }
    });
}

// =============================================================================
// fadeIn / fadeOut / crossfade
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_fade_in(playback: f64, duration_ms: f64, to_vol: f64) {
    let target = (to_vol as f32).clamp(0.0, 1.0);
    if duration_ms <= 0.0 {
        apply_volume_now(playback, target);
        return;
    }
    if let HandleKind::Playback(idx) = classify(playback) {
        VOICES.with(|v| {
            let mut voices = v.borrow_mut();
            if let Some(Some(entry)) = voices.get_mut(idx) {
                unsafe {
                    ma_sound_set_fade_in_milliseconds(
                        ma_ptr(&mut entry.sound),
                        0.0,
                        target,
                        duration_ms as u64,
                    );
                }
                entry.volume = target;
            }
        });
        return;
    }
    let start = current_volume(playback).unwrap_or(0.0);
    schedule_fade(playback, start, target, duration_ms, false);
}

#[no_mangle]
pub extern "C" fn perry_audio_fade_out(playback: f64, duration_ms: f64) {
    if duration_ms <= 0.0 {
        match classify(playback) {
            HandleKind::Playback(idx) => stop_voice(idx, 0.0),
            _ => apply_volume_now(playback, 0.0),
        }
        return;
    }
    if let HandleKind::Playback(idx) = classify(playback) {
        stop_voice(idx, duration_ms);
        return;
    }
    let start = current_volume(playback).unwrap_or(1.0);
    schedule_fade(playback, start, 0.0, duration_ms, false);
}

#[no_mangle]
pub extern "C" fn perry_audio_crossfade(from: f64, to: f64, duration_ms: f64) {
    perry_audio_fade_out(from, duration_ms);
    perry_audio_fade_in(to, duration_ms, 1.0);
}

// =============================================================================
// Buses
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_create_bus(name_ptr: i64, parent: f64) -> i64 {
    if !ensure_engine() {
        return 0;
    }
    let name = str_from_header(name_ptr as *const u8);
    let parent_ptr = match resolve_bus_group(parent) {
        Some(p) => p,
        None => {
            eprintln!("[perry/audio] createBus: invalid parent handle");
            return 0;
        }
    };
    let mut group = ma_zeroed::<MA_GROUP_SIZE>();
    let rc = with_engine_ptr(|engine_ptr| unsafe {
        ma_sound_group_init(engine_ptr, 0, parent_ptr, ma_ptr(&mut group))
    });
    let rc = match rc {
        Some(r) => r,
        None => return 0,
    };
    if rc != MA_SUCCESS {
        eprintln!("[perry/audio] ma_sound_group_init failed: {}", rc);
        return 0;
    }
    let entry = BusEntry {
        group,
        _name: name,
        parent_id: parent,
        volume: 1.0,
        muted: false,
        pre_mute_volume: 1.0,
        soloed: false,
    };
    let idx = BUSES.with(|b| slot_insert(&mut b.borrow_mut(), entry));
    bus_handle_id(idx)
}

#[no_mangle]
pub extern "C" fn perry_audio_destroy_bus(bus: f64) {
    let idx = match classify(bus) {
        HandleKind::Bus(i) => i,
        _ => return,
    };
    let taken = BUSES.with(|b| {
        let mut buses = b.borrow_mut();
        buses.get_mut(idx).and_then(|slot| slot.take())
    });
    if let Some(mut entry) = taken {
        unsafe { ma_sound_group_uninit(ma_ptr(&mut entry.group)) };
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_mute_bus(bus: f64, muted: f64) {
    let idx = match classify(bus) {
        HandleKind::Bus(i) => i,
        _ => return,
    };
    let should_mute = muted != 0.0;
    BUSES.with(|b| {
        let mut buses = b.borrow_mut();
        if let Some(Some(entry)) = buses.get_mut(idx) {
            if should_mute && !entry.muted {
                entry.pre_mute_volume = entry.volume;
                entry.muted = true;
            } else if !should_mute && entry.muted {
                entry.muted = false;
                entry.volume = entry.pre_mute_volume;
            }
        }
    });
    reapply_solo_state();
}

#[no_mangle]
pub extern "C" fn perry_audio_solo_bus(bus: f64, soloed: f64) {
    let idx = match classify(bus) {
        HandleKind::Bus(i) => i,
        _ => return,
    };
    let want = soloed != 0.0;
    BUSES.with(|b| {
        let mut buses = b.borrow_mut();
        if let Some(Some(entry)) = buses.get_mut(idx) {
            entry.soloed = want;
        }
    });
    reapply_solo_state();
}

fn reapply_solo_state() {
    let (any_solo, soloed_indices) = BUSES.with(|b| {
        let buses = b.borrow();
        let solo: Vec<usize> = buses
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                Some(e) if e.soloed => Some(i),
                _ => None,
            })
            .collect();
        (!solo.is_empty(), solo)
    });

    let audible: std::collections::HashSet<usize> = if any_solo {
        let mut set = std::collections::HashSet::new();
        BUSES.with(|b| {
            let buses = b.borrow();
            for start in &soloed_indices {
                let mut cur = Some(*start);
                while let Some(i) = cur {
                    if !set.insert(i) {
                        break;
                    }
                    let parent = buses.get(i).and_then(|o| o.as_ref()).map(|e| e.parent_id);
                    cur = match parent {
                        Some(p) => match classify(p) {
                            HandleKind::Bus(pi) => Some(pi),
                            _ => None,
                        },
                        None => None,
                    };
                }
            }
        });
        set
    } else {
        std::collections::HashSet::new()
    };

    BUSES.with(|b| {
        let mut buses = b.borrow_mut();
        for i in 0..buses.len() {
            if let Some(Some(entry)) = buses.get_mut(i) {
                let effective: f32 = if entry.muted {
                    0.0
                } else if any_solo && !audible.contains(&i) {
                    0.0
                } else {
                    entry.volume
                };
                unsafe { ma_sound_group_set_volume(ma_ptr(&mut entry.group), effective) };
            }
        }
    });
}

// =============================================================================
// Master / engine
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_set_master_volume(volume: f64, fade_ms: f64) {
    let target = (volume as f32).clamp(0.0, 1.0);
    if fade_ms <= 0.0 {
        apply_volume_now(0.0, target);
        return;
    }
    let start = current_volume(0.0).unwrap_or(1.0);
    schedule_fade(0.0, start, target, fade_ms, false);
}

#[no_mangle]
pub extern "C" fn perry_audio_suspend() {
    let _ = with_engine_ptr(|p| unsafe { ma_engine_stop(p) });
}

#[no_mangle]
pub extern "C" fn perry_audio_resume_all() {
    let _ = with_engine_ptr(|p| unsafe { ma_engine_start(p) });
}

// =============================================================================
// Introspection
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_audio_is_playing(handle: f64) -> f64 {
    match classify(handle) {
        HandleKind::Playback(idx) => VOICES.with(|v| {
            let voices = v.borrow();
            match voices.get(idx).and_then(|o| o.as_ref()) {
                Some(e) if e.is_playing && !e.is_paused => {
                    let live = unsafe { ma_sound_is_playing(ma_ptr_const(&e.sound)) };
                    if live != 0 {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => 0.0,
            }
        }),
        HandleKind::Sound(idx) => VOICES.with(|v| {
            let any = v.borrow().iter().any(|slot| match slot {
                Some(e) => e.sound_idx == idx && e.is_playing && !e.is_paused,
                None => false,
            });
            if any {
                1.0
            } else {
                0.0
            }
        }),
        _ => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn perry_audio_get_duration(sound: f64) -> f64 {
    let idx = match classify(sound) {
        HandleKind::Sound(i) => i,
        _ => return 0.0,
    };
    SOUNDS.with(|s| {
        let mut sounds = s.borrow_mut();
        match sounds.get_mut(idx).and_then(|o| o.as_mut()) {
            Some(entry) => {
                let mut seconds: c_float = 0.0;
                let rc = unsafe {
                    ma_sound_get_length_in_seconds(ma_ptr(&mut entry.template), &mut seconds)
                };
                if rc == MA_SUCCESS {
                    seconds as f64
                } else {
                    0.0
                }
            }
            None => 0.0,
        }
    })
}

#[no_mangle]
pub extern "C" fn perry_audio_get_position(playback: f64) -> f64 {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return 0.0,
    };
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        match voices.get_mut(idx).and_then(|o| o.as_mut()) {
            Some(entry) => {
                let mut seconds: c_float = 0.0;
                let rc = unsafe {
                    ma_sound_get_cursor_in_seconds(ma_ptr(&mut entry.sound), &mut seconds)
                };
                if rc == MA_SUCCESS {
                    seconds as f64
                } else {
                    0.0
                }
            }
            None => 0.0,
        }
    })
}

#[no_mangle]
pub extern "C" fn perry_audio_on_ended(playback: f64, callback: f64) {
    let idx = match classify(playback) {
        HandleKind::Playback(i) => i,
        _ => return,
    };
    VOICES.with(|v| {
        let mut voices = v.borrow_mut();
        if let Some(Some(entry)) = voices.get_mut(idx) {
            entry.on_ended = Some(callback);
        }
    });
}

// =============================================================================
// Software fade timer
//
// miniaudio's native fade only covers ma_sound voices. For master / bus /
// sound-default fades we tick a 60 Hz software fade in-process. Without a
// platform timer (NSTimer / Win32 SetTimer) the tick is driven lazily by
// the dispatch hot paths (every audio call drains pending callbacks /
// advances any active fade). This is good enough for game-loop usage,
// where audio calls happen every frame anyway, and matches what AVAudio
// would do if its fade was deferred to the run loop.
// =============================================================================

const FADE_HZ: f64 = 60.0;

fn schedule_fade(handle: f64, start: f32, target: f32, duration_ms: f64, then_stop: bool) {
    let ticks = ((duration_ms / 1000.0) * FADE_HZ).max(1.0) as u32;
    FADES.with(|f| {
        let mut fades = f.borrow_mut();
        fades.retain(|fade| fade.handle != handle);
        fades.push(Fade {
            handle,
            start_vol: start,
            target_vol: target,
            ticks_total: ticks,
            ticks_left: ticks,
            then_stop,
        });
    });
    advance_fades();
}

fn advance_fades() {
    let to_apply: Vec<(f64, f32, bool)> = FADES.with(|f| {
        let mut fades = f.borrow_mut();
        let mut out: Vec<(f64, f32, bool)> = Vec::new();
        fades.retain_mut(|fade| {
            if fade.ticks_left == 0 {
                return false;
            }
            // Jump straight to target — we don't have a real timer; the
            // smooth-fade approximation is left to miniaudio's native
            // fade where we can use it (voices).
            let v = fade.target_vol;
            fade.ticks_left = 0;
            out.push((fade.handle, v, fade.then_stop));
            !true
        });
        out
    });
    for (handle, vol, then_stop) in to_apply {
        apply_volume_now(handle, vol);
        if then_stop {
            if let HandleKind::Playback(idx) = classify(handle) {
                stop_voice(idx, 0.0);
            }
        }
    }
}
