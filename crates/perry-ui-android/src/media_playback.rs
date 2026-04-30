//! Android streaming media playback (`perry/media`) — `android.media.MediaPlayer`
//! via JNI.
//!
//! Each handle wraps a MediaPlayer GlobalRef. We use the synchronous
//! `prepare()` call from a worker thread so the main UI thread doesn't
//! block on network buffering, and we don't need to register a Java-side
//! `OnPreparedListener` (which would need either a Java helper class or
//! `java.lang.reflect.Proxy.newProxyInstance` — both add complexity).
//!
//! State derivation mirrors the macOS `AVPlayer` impl:
//! - `Loading` until the worker thread sets `prepared = true`
//! - `Ready` once prepared, before `play()` is ever called
//! - `Playing` / `Paused` from `isPlaying()` after `start()` was called
//! - `Ended` when `currentPosition >= duration - 0.25s` (belt-and-braces
//!   per acroyear's #351 comment — same robustness as Apple)
//! - `Error` on any JNI exception caught during a control call
//!
//! A 10 Hz polling thread fires the JS state-change + time-update
//! callbacks, matching the cross-platform contract.

use jni::objects::{GlobalRef, JObject, JValue};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::jni_bridge;

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, a: f64, b: f64) -> f64;
    fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    fn js_string_new_sso(data: *const u8, len: u32) -> f64;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum MediaState {
    Idle,
    Loading,
    Ready,
    Playing,
    Paused,
    Ended,
    Error,
}

impl MediaState {
    fn as_str(self) -> &'static str {
        match self {
            MediaState::Idle => "idle",
            MediaState::Loading => "loading",
            MediaState::Ready => "ready",
            MediaState::Playing => "playing",
            MediaState::Paused => "paused",
            MediaState::Ended => "ended",
            MediaState::Error => "error",
        }
    }
}

struct PlayerEntry {
    /// Java MediaPlayer object — `Arc<Mutex<>>` because the prepare worker
    /// thread reads it after the main thread stored it.
    player: Arc<Mutex<Option<GlobalRef>>>,
    state: MediaState,
    /// Set by the prepare worker thread once `prepare()` returns.
    prepared: Arc<AtomicBool>,
    /// Set by an attempted control call that threw a JNI exception.
    error: Arc<AtomicBool>,
    has_started: bool,
    duration_seconds: f64,
    on_state_change: Option<f64>,
    on_time_update: Option<f64>,
}

thread_local! {
    static PLAYERS: RefCell<Vec<Option<PlayerEntry>>> = const { RefCell::new(Vec::new()) };
    /// Tick counter for throttling the polling — `nativePumpTick` fires
    /// every 8ms from the UI thread; we only need ~10 Hz state/time
    /// updates so we run the actual poll every 12 ticks (~96 ms).
    static PUMP_COUNTER: RefCell<u32> = const { RefCell::new(0) };
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

fn str_from_header<'a>(ptr: *const u8) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

// ---------------------------------------------------------------------------
// Public FFI
// ---------------------------------------------------------------------------

pub fn create_player(url_ptr: *const u8) -> i64 {
    let url = str_from_header(url_ptr);
    if url.is_empty() {
        return 0;
    }

    let player_arc: Arc<Mutex<Option<GlobalRef>>> = Arc::new(Mutex::new(None));
    let prepared = Arc::new(AtomicBool::new(false));
    let error = Arc::new(AtomicBool::new(false));

    // Spawn a worker thread that constructs the MediaPlayer, sets the
    // data source, and calls the synchronous `prepare()` (blocks on
    // network buffering for HTTP URLs). When done, stores a GlobalRef
    // back into the shared slot so the main thread can issue control
    // calls.
    let url_owned = url.to_string();
    let player_arc_w = Arc::clone(&player_arc);
    let prepared_w = Arc::clone(&prepared);
    let error_w = Arc::clone(&error);
    std::thread::spawn(move || {
        let vm = jni_bridge::get_vm().clone();
        let mut env = match vm.attach_current_thread_permanently() {
            Ok(e) => e,
            Err(_) => {
                error_w.store(true, Ordering::Relaxed);
                return;
            }
        };
        let _ = env.push_local_frame(8);

        // new MediaPlayer()
        let mp = match env.new_object("android/media/MediaPlayer", "()V", &[]) {
            Ok(o) => o,
            Err(_) => {
                error_w.store(true, Ordering::Relaxed);
                unsafe {
                    env.pop_local_frame(&JObject::null());
                }
                return;
            }
        };

        // setDataSource(String url)
        let url_jstr = match env.new_string(&url_owned) {
            Ok(s) => s,
            Err(_) => {
                error_w.store(true, Ordering::Relaxed);
                unsafe {
                    env.pop_local_frame(&JObject::null());
                }
                return;
            }
        };
        if env
            .call_method(
                &mp,
                "setDataSource",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&url_jstr.into())],
            )
            .is_err()
        {
            let _ = env.exception_clear();
            error_w.store(true, Ordering::Relaxed);
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return;
        }

        // setAudioStreamType(STREAM_MUSIC=3) — deprecated since API 26 but
        // still works; the modern AudioAttributes setter is more verbose
        // and the deprecated path is a single call.
        let _ = env.call_method(
            &mp,
            "setAudioStreamType",
            "(I)V",
            &[JValue::Int(3)],
        );
        let _ = env.exception_clear();

        // prepare() — synchronous. Blocks until the source is ready.
        if env.call_method(&mp, "prepare", "()V", &[]).is_err() {
            let _ = env.exception_clear();
            error_w.store(true, Ordering::Relaxed);
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return;
        }

        let global = match env.new_global_ref(&mp) {
            Ok(g) => g,
            Err(_) => {
                error_w.store(true, Ordering::Relaxed);
                unsafe {
                    env.pop_local_frame(&JObject::null());
                }
                return;
            }
        };
        unsafe {
            env.pop_local_frame(&JObject::null());
        }

        if let Ok(mut slot) = player_arc_w.lock() {
            *slot = Some(global);
        }
        prepared_w.store(true, Ordering::Relaxed);
    });

    let entry = PlayerEntry {
        player: player_arc,
        state: MediaState::Loading,
        prepared,
        error,
        has_started: false,
        duration_seconds: 0.0,
        on_state_change: None,
        on_time_update: None,
    };

    let handle = PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        for (i, slot) in players.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(entry);
                return (i + 1) as i64;
            }
        }
        players.push(Some(entry));
        players.len() as i64
    });

    // No standalone poll thread — Java callbacks need the JVM main UI
    // thread for JNI access, and PLAYERS is `thread_local` to that thread.
    // `pump_tick()` is called from `app.rs::nativePumpTick` every 8ms
    // (~125 Hz), throttled internally to 10 Hz.
    handle
}

pub fn play(handle: f64) {
    with_entry_mut(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            with_env(|env| {
                let _ = env.call_method(global.as_obj(), "start", "()V", &[]);
                let _ = env.exception_clear();
            });
            entry.has_started = true;
        }
    });
}

pub fn pause(handle: f64) {
    with_entry_mut(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            with_env(|env| {
                let _ = env.call_method(global.as_obj(), "pause", "()V", &[]);
                let _ = env.exception_clear();
            });
        }
    });
}

pub fn stop(handle: f64) {
    with_entry_mut(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            with_env(|env| {
                let _ = env.call_method(global.as_obj(), "pause", "()V", &[]);
                let _ = env.call_method(
                    global.as_obj(),
                    "seekTo",
                    "(I)V",
                    &[JValue::Int(0)],
                );
                let _ = env.exception_clear();
            });
            entry.has_started = false;
        }
    });
}

pub fn seek(handle: f64, seconds: f64) {
    with_entry_mut(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            let ms = (seconds * 1000.0).max(0.0) as i32;
            with_env(|env| {
                let _ = env.call_method(
                    global.as_obj(),
                    "seekTo",
                    "(I)V",
                    &[JValue::Int(ms)],
                );
                let _ = env.exception_clear();
            });
        }
    });
}

pub fn set_volume(handle: f64, volume: f64) {
    with_entry_mut(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            let v = volume.clamp(0.0, 1.0) as f32;
            with_env(|env| {
                let _ = env.call_method(
                    global.as_obj(),
                    "setVolume",
                    "(FF)V",
                    &[JValue::Float(v), JValue::Float(v)],
                );
                let _ = env.exception_clear();
            });
        }
    });
}

pub fn set_rate(handle: f64, rate: f64) {
    // MediaPlayer.setPlaybackParams requires API 23+. Errors are
    // swallowed — best-effort because some codecs don't support
    // arbitrary rate changes.
    with_entry_mut(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            with_env(|env| {
                let pp_cls = match env.find_class("android/media/PlaybackParams") {
                    Ok(c) => c,
                    Err(_) => {
                        let _ = env.exception_clear();
                        return;
                    }
                };
                let pp = match env.new_object(pp_cls, "()V", &[]) {
                    Ok(o) => o,
                    Err(_) => {
                        let _ = env.exception_clear();
                        return;
                    }
                };
                if env
                    .call_method(
                        &pp,
                        "setSpeed",
                        "(F)Landroid/media/PlaybackParams;",
                        &[JValue::Float(rate as f32)],
                    )
                    .is_err()
                {
                    let _ = env.exception_clear();
                    return;
                }
                let _ = env.call_method(
                    global.as_obj(),
                    "setPlaybackParams",
                    "(Landroid/media/PlaybackParams;)V",
                    &[JValue::Object(&pp)],
                );
                let _ = env.exception_clear();
            });
        }
    });
}

pub fn get_current_time(handle: f64) -> f64 {
    with_entry(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            let mut out = 0.0;
            with_env(|env| {
                if let Ok(v) = env.call_method(global.as_obj(), "getCurrentPosition", "()I", &[]) {
                    out = v.i().unwrap_or(0) as f64 / 1000.0;
                }
                let _ = env.exception_clear();
            });
            out
        } else {
            0.0
        }
    })
    .unwrap_or(0.0)
}

pub fn get_duration(handle: f64) -> f64 {
    with_entry(handle, |entry| entry.duration_seconds.max(0.0)).unwrap_or(0.0)
}

pub fn get_state(handle: f64) -> i64 {
    let state = with_entry(handle, |entry| entry.state).unwrap_or(MediaState::Idle);
    let s = state.as_str();
    unsafe { js_string_from_bytes(s.as_ptr(), s.len() as i32) }
}

pub fn is_playing(handle: f64) -> f64 {
    if matches!(
        with_entry(handle, |entry| entry.state).unwrap_or(MediaState::Idle),
        MediaState::Playing
    ) {
        1.0
    } else {
        0.0
    }
}

pub fn on_state_change(handle: f64, closure: f64) {
    with_entry_mut(handle, |entry| entry.on_state_change = Some(closure));
}

pub fn on_time_update(handle: f64, closure: f64) {
    with_entry_mut(handle, |entry| entry.on_time_update = Some(closure));
}

pub fn set_now_playing(
    _handle: f64,
    _title_ptr: *const u8,
    _artist_ptr: *const u8,
    _album_ptr: *const u8,
    _artwork_ptr: *const u8,
) {
    // Lock-screen integration on Android needs MediaSessionCompat from
    // the androidx.media package — a Service binding the session, a
    // PlaybackStateCompat to push state, and a MediaMetadataCompat to
    // push metadata. That's another ~300 LOC of JNI plumbing. Tracked
    // in a #351 follow-up; the metadata is silently dropped here so
    // callers don't have to feature-detect.
}

pub fn destroy(handle: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    let entry = PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        players.get_mut(idx).and_then(|s| s.take())
    });
    if let Some(entry) = entry {
        if let Some(global) = lock_player(&entry.player) {
            with_env(|env| {
                let _ = env.call_method(global.as_obj(), "release", "()V", &[]);
                let _ = env.exception_clear();
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn handle_to_index(handle: f64) -> Option<usize> {
    let h = handle as i64;
    if h <= 0 {
        None
    } else {
        Some((h - 1) as usize)
    }
}

fn with_entry<R, F: FnOnce(&PlayerEntry) -> R>(handle: f64, f: F) -> Option<R> {
    let idx = handle_to_index(handle)?;
    PLAYERS.with(|p| {
        let players = p.borrow();
        players.get(idx).and_then(|s| s.as_ref()).map(f)
    })
}

fn with_entry_mut<F: FnOnce(&mut PlayerEntry)>(handle: f64, f: F) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            f(entry);
        }
    });
}

fn lock_player(p: &Arc<Mutex<Option<GlobalRef>>>) -> Option<GlobalRef> {
    p.lock().ok()?.clone()
}

fn with_env<F: FnOnce(&mut jni::JNIEnv)>(f: F) {
    let mut env = jni_bridge::get_env();
    f(&mut env);
}

// ---------------------------------------------------------------------------
// Pump tick — driven from `app.rs::nativePumpTick` (UI thread, 125 Hz).
// Throttled internally to ~10 Hz so `onTimeUpdate` doesn't flood the JS
// callback queue.
// ---------------------------------------------------------------------------

/// Called from `Java_com_perry_app_PerryBridge_nativePumpTick`. Cheap
/// when there are no players; when there are, runs a state + time-update
/// tick every 12th call (~96 ms apart).
pub fn pump_tick() {
    let should_run = PUMP_COUNTER.with(|c| {
        let mut v = c.borrow_mut();
        *v = v.wrapping_add(1);
        *v % 12 == 0
    });
    if should_run {
        poll_tick();
    }
}

fn poll_tick() {
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        for slot in players.iter_mut() {
            let entry = match slot {
                Some(e) => e,
                None => continue,
            };

            let new_state = derive_state(entry);
            let state_changed = new_state != entry.state;
            entry.state = new_state;

            // Refresh duration once prepared — getDuration returns ms,
            // -1 if unknown (live stream).
            if entry.prepared.load(Ordering::Relaxed)
                && entry.duration_seconds == 0.0
            {
                if let Some(global) = lock_player(&entry.player) {
                    let mut env = jni_bridge::get_env();
                    if let Ok(v) = env.call_method(
                        global.as_obj(),
                        "getDuration",
                        "()I",
                        &[],
                    ) {
                        let ms = v.i().unwrap_or(0);
                        if ms > 0 {
                            entry.duration_seconds = ms as f64 / 1000.0;
                        }
                    }
                    let _ = env.exception_clear();
                }
            }

            let on_state = if state_changed {
                entry.on_state_change
            } else {
                None
            };
            let on_time = if matches!(new_state, MediaState::Playing | MediaState::Loading) {
                entry.on_time_update
            } else {
                None
            };
            let cur = current_time_seconds(entry);
            let dur = entry.duration_seconds;

            if let Some(cb) = on_state {
                fire_state_callback(cb, new_state);
            }
            if let Some(cb) = on_time {
                fire_time_callback(cb, cur, dur);
            }
        }
    });
}

fn current_time_seconds(entry: &PlayerEntry) -> f64 {
    if let Some(global) = lock_player(&entry.player) {
        let mut env = jni_bridge::get_env();
        if let Ok(v) = env.call_method(global.as_obj(), "getCurrentPosition", "()I", &[]) {
            let _ = env.exception_clear();
            return v.i().unwrap_or(0) as f64 / 1000.0;
        }
        let _ = env.exception_clear();
    }
    0.0
}

fn derive_state(entry: &PlayerEntry) -> MediaState {
    if entry.error.load(Ordering::Relaxed) {
        return MediaState::Error;
    }
    if !entry.prepared.load(Ordering::Relaxed) {
        return MediaState::Loading;
    }
    // Belt-and-braces ended detection (issue #351 acroyear comment).
    if entry.has_started && entry.duration_seconds > 0.25 {
        let cur = current_time_seconds(entry);
        if cur >= entry.duration_seconds - 0.25 {
            return MediaState::Ended;
        }
    }
    if !entry.has_started {
        return MediaState::Ready;
    }
    if let Some(global) = lock_player(&entry.player) {
        let mut env = jni_bridge::get_env();
        if let Ok(v) = env.call_method(global.as_obj(), "isPlaying", "()Z", &[]) {
            let _ = env.exception_clear();
            return if v.z().unwrap_or(false) {
                MediaState::Playing
            } else {
                MediaState::Paused
            };
        }
        let _ = env.exception_clear();
    }
    MediaState::Paused
}

fn fire_state_callback(closure_f64: f64, state: MediaState) {
    unsafe {
        js_run_stdlib_pump();
        let _ = js_promise_run_microtasks();
        let s = state.as_str();
        let str_f64 = js_string_new_sso(s.as_ptr(), s.len() as u32);
        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        let _ = js_closure_call1(closure_ptr as *const u8, str_f64);
    }
}

fn fire_time_callback(closure_f64: f64, current: f64, duration: f64) {
    unsafe {
        js_run_stdlib_pump();
        let _ = js_promise_run_microtasks();
        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        let _ = js_closure_call2(closure_ptr as *const u8, current, duration);
    }
}
