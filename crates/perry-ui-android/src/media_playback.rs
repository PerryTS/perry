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
use std::sync::{Arc, Mutex, OnceLock};

use crate::jni_bridge;

/// Process-wide MediaSessionCompat — lazily constructed on first
/// `set_now_playing` call. The session is shared across all players
/// because MediaSessionCompat surfaces a single Now Playing slot to the
/// system (lock-screen / Bluetooth / Wear OS), matching the Apple
/// MPNowPlayingInfoCenter semantics: most recent `setNowPlaying` wins.
static MEDIA_SESSION: OnceLock<Mutex<Option<GlobalRef>>> = OnceLock::new();

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
        let _ = env.call_method(&mp, "setAudioStreamType", "(I)V", &[JValue::Int(3)]);
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
    if MEDIA_SESSION.get().is_some() {
        push_playback_state(MediaState::Playing, get_current_time(handle));
    }
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
    if MEDIA_SESSION.get().is_some() {
        push_playback_state(MediaState::Paused, get_current_time(handle));
    }
}

pub fn stop(handle: f64) {
    with_entry_mut(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            with_env(|env| {
                let _ = env.call_method(global.as_obj(), "pause", "()V", &[]);
                let _ = env.call_method(global.as_obj(), "seekTo", "(I)V", &[JValue::Int(0)]);
                let _ = env.exception_clear();
            });
            entry.has_started = false;
        }
    });
    if MEDIA_SESSION.get().is_some() {
        push_playback_state(MediaState::Idle, 0.0);
    }
}

pub fn seek(handle: f64, seconds: f64) {
    with_entry_mut(handle, |entry| {
        if let Some(global) = lock_player(&entry.player) {
            let ms = (seconds * 1000.0).max(0.0) as i32;
            with_env(|env| {
                let _ = env.call_method(global.as_obj(), "seekTo", "(I)V", &[JValue::Int(ms)]);
                let _ = env.exception_clear();
            });
        }
    });
    if MEDIA_SESSION.get().is_some() {
        let state = with_entry(handle, |e| e.state).unwrap_or(MediaState::Idle);
        push_playback_state(state, seconds);
    }
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
    handle: f64,
    title_ptr: *const u8,
    artist_ptr: *const u8,
    album_ptr: *const u8,
    artwork_ptr: *const u8,
) {
    // The handle is currently advisory — MediaSessionCompat is a
    // process-wide singleton, so the most recent setNowPlaying wins.
    // Mirrors the Apple semantics; multi-player apps that need an
    // explicit active player should manage that themselves.
    let _ = handle;

    let title = str_from_header(title_ptr).to_string();
    let artist = str_from_header(artist_ptr).to_string();
    let album = str_from_header(album_ptr).to_string();
    let artwork = str_from_header(artwork_ptr).to_string();

    let session = match ensure_session() {
        Some(s) => s,
        None => return,
    };

    with_env(|env| {
        let _ = env.push_local_frame(16);

        // MediaMetadataCompat.Builder — putString returns the builder,
        // build() returns the metadata.
        let builder = match env.new_object(
            "android/support/v4/media/MediaMetadataCompat$Builder",
            "()V",
            &[],
        ) {
            Ok(b) => b,
            Err(_) => {
                let _ = env.exception_clear();
                unsafe {
                    env.pop_local_frame(&JObject::null());
                }
                return;
            }
        };

        let put_string = |env: &mut jni::JNIEnv, key: &str, val: &str| {
            if val.is_empty() {
                return;
            }
            let k = match env.new_string(key) {
                Ok(s) => s,
                Err(_) => {
                    let _ = env.exception_clear();
                    return;
                }
            };
            let v = match env.new_string(val) {
                Ok(s) => s,
                Err(_) => {
                    let _ = env.exception_clear();
                    return;
                }
            };
            let _ = env.call_method(
                &builder,
                "putString",
                "(Ljava/lang/String;Ljava/lang/String;)Landroid/support/v4/media/MediaMetadataCompat$Builder;",
                &[JValue::Object(&k.into()), JValue::Object(&v.into())],
            );
            let _ = env.exception_clear();
        };

        put_string(env, "android.media.metadata.TITLE", &title);
        put_string(env, "android.media.metadata.ARTIST", &artist);
        put_string(env, "android.media.metadata.ALBUM", &album);

        if !artwork.is_empty() {
            if let Some(bitmap) = decode_artwork(env, &artwork) {
                if let Ok(key) = env.new_string("android.media.metadata.ART") {
                    let _ = env.call_method(
                        &builder,
                        "putBitmap",
                        "(Ljava/lang/String;Landroid/graphics/Bitmap;)Landroid/support/v4/media/MediaMetadataCompat$Builder;",
                        &[JValue::Object(&key.into()), JValue::Object(&bitmap)],
                    );
                    let _ = env.exception_clear();
                }
            }
        }

        let metadata = match env.call_method(
            &builder,
            "build",
            "()Landroid/support/v4/media/MediaMetadataCompat;",
            &[],
        ) {
            Ok(v) => match v.l() {
                Ok(o) => o,
                Err(_) => {
                    unsafe {
                        env.pop_local_frame(&JObject::null());
                    }
                    return;
                }
            },
            Err(_) => {
                let _ = env.exception_clear();
                unsafe {
                    env.pop_local_frame(&JObject::null());
                }
                return;
            }
        };

        let _ = env.call_method(
            session.as_obj(),
            "setMetadata",
            "(Landroid/support/v4/media/MediaMetadataCompat;)V",
            &[JValue::Object(&metadata)],
        );
        let _ = env.exception_clear();

        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    });
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
            if entry.prepared.load(Ordering::Relaxed) && entry.duration_seconds == 0.0 {
                if let Some(global) = lock_player(&entry.player) {
                    let mut env = jni_bridge::get_env();
                    if let Ok(v) = env.call_method(global.as_obj(), "getDuration", "()I", &[]) {
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
            // Keep the system UI in sync — only push when state flipped
            // to avoid hammering the session every 96ms with the same
            // state. Position is taken from the current poll.
            if state_changed && MEDIA_SESSION.get().is_some() {
                push_playback_state(new_state, cur);
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

// ---------------------------------------------------------------------------
// MediaSessionCompat — lock-screen / Bluetooth / Wear OS integration.
// ---------------------------------------------------------------------------

/// Lazily construct the process-wide MediaSessionCompat. Returns the
/// session GlobalRef on success. Setting active=true is required for
/// the session to receive media-button events and surface metadata to
/// the system UI.
fn ensure_session() -> Option<GlobalRef> {
    let cell = MEDIA_SESSION.get_or_init(|| Mutex::new(None));
    {
        if let Ok(slot) = cell.lock() {
            if let Some(ref s) = *slot {
                return Some(s.clone());
            }
        }
    }

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    let activity = crate::widgets::get_activity(&mut env);
    let tag = match env.new_string("perry-media") {
        Ok(s) => s,
        Err(_) => {
            let _ = env.exception_clear();
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return None;
        }
    };

    let session = match env.new_object(
        "android/support/v4/media/session/MediaSessionCompat",
        "(Landroid/content/Context;Ljava/lang/String;)V",
        &[JValue::Object(&activity), JValue::Object(&tag.into())],
    ) {
        Ok(o) => o,
        Err(_) => {
            let _ = env.exception_clear();
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return None;
        }
    };

    let _ = env.call_method(&session, "setActive", "(Z)V", &[JValue::Bool(1)]);
    let _ = env.exception_clear();

    // Wire the headphone/media-button callback. The Java helper class
    // PerryMediaSessionCallback extends MediaSessionCompat.Callback and
    // routes onPlay / onPause / onStop / onSeekTo to the native exports
    // below.
    if let Ok(callback) = env.new_object("com/perry/app/PerryMediaSessionCallback", "()V", &[]) {
        let _ = env.call_method(
            &session,
            "setCallback",
            "(Landroid/support/v4/media/session/MediaSessionCompat$Callback;)V",
            &[JValue::Object(&callback)],
        );
        let _ = env.exception_clear();
    } else {
        // The Java helper class isn't compiled into this APK — the
        // session still works for metadata / state, but headphone keys
        // will fall back to system defaults.
        let _ = env.exception_clear();
    }

    let global = match env.new_global_ref(&session) {
        Ok(g) => g,
        Err(_) => {
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return None;
        }
    };
    unsafe {
        env.pop_local_frame(&JObject::null());
    }

    if let Ok(mut slot) = cell.lock() {
        *slot = Some(global.clone());
    }
    Some(global)
}

/// Decode an artwork URL into an Android Bitmap. Returns the local-ref
/// JObject on success; failures (network error, decode failure, missing
/// file) are silently swallowed so set_now_playing still publishes the
/// title/artist/album metadata.
fn decode_artwork<'a>(env: &mut jni::JNIEnv<'a>, url: &str) -> Option<JObject<'a>> {
    let factory = "android/graphics/BitmapFactory";

    if url.starts_with("http://") || url.starts_with("https://") {
        let url_jstr = env.new_string(url).ok()?;
        let url_obj = env
            .new_object(
                "java/net/URL",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&url_jstr.into())],
            )
            .ok()?;
        let stream = match env.call_method(&url_obj, "openStream", "()Ljava/io/InputStream;", &[]) {
            Ok(v) => v.l().ok()?,
            Err(_) => {
                let _ = env.exception_clear();
                return None;
            }
        };
        let bitmap = match env.call_static_method(
            factory,
            "decodeStream",
            "(Ljava/io/InputStream;)Landroid/graphics/Bitmap;",
            &[JValue::Object(&stream)],
        ) {
            Ok(v) => v.l().ok()?,
            Err(_) => {
                let _ = env.exception_clear();
                return None;
            }
        };
        let _ = env.call_method(&stream, "close", "()V", &[]);
        let _ = env.exception_clear();
        if bitmap.is_null() {
            None
        } else {
            Some(bitmap)
        }
    } else {
        let path = url.strip_prefix("file://").unwrap_or(url);
        let path_jstr = env.new_string(path).ok()?;
        let bitmap = match env.call_static_method(
            factory,
            "decodeFile",
            "(Ljava/lang/String;)Landroid/graphics/Bitmap;",
            &[JValue::Object(&path_jstr.into())],
        ) {
            Ok(v) => v.l().ok()?,
            Err(_) => {
                let _ = env.exception_clear();
                return None;
            }
        };
        if bitmap.is_null() {
            None
        } else {
            Some(bitmap)
        }
    }
}

/// Build and push a PlaybackStateCompat reflecting the current player
/// state. Position is in seconds; converted to ms for the Java API.
/// State constants from PlaybackStateCompat: STOPPED=1, PAUSED=2,
/// PLAYING=3, BUFFERING=6, ERROR=7. Idle/Ready/Ended map to STOPPED.
fn push_playback_state(state: MediaState, position_seconds: f64) {
    let session = match MEDIA_SESSION
        .get()
        .and_then(|m| m.lock().ok().and_then(|s| s.clone()))
    {
        Some(s) => s,
        None => return,
    };

    let state_code: i32 = match state {
        MediaState::Playing => 3,
        MediaState::Paused => 2,
        MediaState::Loading => 6,
        MediaState::Error => 7,
        MediaState::Idle | MediaState::Ready | MediaState::Ended => 1,
    };
    let position_ms = (position_seconds * 1000.0).max(0.0) as i64;

    // ACTION_STOP=1, PLAY_PAUSE=512, PLAY=4, PAUSE=2, SEEK_TO=256,
    // SKIP_TO_NEXT=32, SKIP_TO_PREVIOUS=16. Bluetooth / Auto / Wear OS
    // greys out buttons whose action bit isn't set, so advertise the
    // full transport-control set we actually wire on the callback.
    let actions: i64 = 4 | 2 | 512 | 256 | 1 | 32 | 16;

    with_env(|env| {
        let _ = env.push_local_frame(8);
        let builder = match env.new_object(
            "android/support/v4/media/session/PlaybackStateCompat$Builder",
            "()V",
            &[],
        ) {
            Ok(b) => b,
            Err(_) => {
                let _ = env.exception_clear();
                unsafe {
                    env.pop_local_frame(&JObject::null());
                }
                return;
            }
        };
        let _ = env.call_method(
            &builder,
            "setState",
            "(IJF)Landroid/support/v4/media/session/PlaybackStateCompat$Builder;",
            &[
                JValue::Int(state_code),
                JValue::Long(position_ms),
                JValue::Float(1.0),
            ],
        );
        let _ = env.exception_clear();
        let _ = env.call_method(
            &builder,
            "setActions",
            "(J)Landroid/support/v4/media/session/PlaybackStateCompat$Builder;",
            &[JValue::Long(actions)],
        );
        let _ = env.exception_clear();
        let built = match env.call_method(
            &builder,
            "build",
            "()Landroid/support/v4/media/session/PlaybackStateCompat;",
            &[],
        ) {
            Ok(v) => v.l().ok(),
            Err(_) => {
                let _ = env.exception_clear();
                None
            }
        };
        if let Some(ps) = built {
            let _ = env.call_method(
                session.as_obj(),
                "setPlaybackState",
                "(Landroid/support/v4/media/session/PlaybackStateCompat;)V",
                &[JValue::Object(&ps)],
            );
            let _ = env.exception_clear();
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    });
}

/// Find the first live player handle. Mirrors the macOS
/// MPRemoteCommandCenter convention: media-button events from the
/// system route to whichever player is current. Multi-player apps that
/// need an explicit active player should manage their own state.
fn first_active_player_handle() -> Option<f64> {
    PLAYERS.with(|p| {
        let players = p.borrow();
        for (i, slot) in players.iter().enumerate() {
            if slot.is_some() {
                return Some((i + 1) as f64);
            }
        }
        None
    })
}

// ---------------------------------------------------------------------------
// JNI exports invoked from PerryMediaSessionCallback.java when the user
// taps headphone / Bluetooth / Wear OS / lock-screen transport controls.
// All four are no-ops if there's no live player.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryMediaSessionCallback_nativeMediaSessionPlay(
    _env: jni::JNIEnv,
    _class: JObject,
) {
    if let Some(h) = first_active_player_handle() {
        play(h);
    }
}

#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryMediaSessionCallback_nativeMediaSessionPause(
    _env: jni::JNIEnv,
    _class: JObject,
) {
    if let Some(h) = first_active_player_handle() {
        pause(h);
    }
}

#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryMediaSessionCallback_nativeMediaSessionStop(
    _env: jni::JNIEnv,
    _class: JObject,
) {
    if let Some(h) = first_active_player_handle() {
        stop(h);
    }
}

#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryMediaSessionCallback_nativeMediaSessionSeekTo(
    _env: jni::JNIEnv,
    _class: JObject,
    position_ms: jni::sys::jlong,
) {
    if let Some(h) = first_active_player_handle() {
        seek(h, (position_ms as f64) / 1000.0);
    }
}
