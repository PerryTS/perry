//! HarmonyOS streaming media playback (`perry/media`) — `@ohos.multimedia.media.AVPlayer`
//! via NAPI drain-queue bridge.
//!
//! Same architectural shape as `arkts_callbacks.rs`'s showToast / setText:
//! Perry's compiled `.so` can't reach `@ohos.multimedia.media` directly, so
//! every `perry_media_*` FFI thunk records an *intent* into one of three
//! Mutex-protected drain queues. ArkTS-side glue (emitted by
//! `perry-codegen-arkts::wrap_index_page` into `Index.ets`) polls the
//! queues on a 100 ms timer, dispatches each intent against an AVPlayer
//! instance, and pushes state back into the runtime via
//! `napi_push_media_state(handle, state, current, duration)`.
//!
//! Lookups (`getCurrentTime` / `getDuration` / `getState` / `isPlaying`)
//! read from `MEDIA_STATE_INBOX` — the most-recently-pushed state per
//! handle. Defaults are `(MediaState::Idle, 0.0, 0.0)` until ArkTS pushes
//! a first observation. The 100 ms drain cadence is fast enough that
//! reading the inbox is effectively live.
//!
//! Lock-screen (`setNowPlaying`) routes through its own queue and is
//! drained against `@ohos.multimedia.avsession`. Best-effort — if the
//! ArkTS-side AVSession plumbing turns out to be more involved than the
//! AVPlayer side (custom backgroundService manifest entries etc.), the
//! glue can no-op the avsession dispatch and the queue silently drains
//! to nothing. Tracked under #369 for follow-up.
//!
//! Gating: this entire module is `#[cfg(feature = "ohos-napi")]`. On
//! every other target the `perry_media_*` symbols come from the platform
//! UI crate (perry-ui-macos / perry-ui-android / etc).

#![cfg(feature = "ohos-napi")]

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use crate::closure::{js_closure_call1, js_closure_call2, ClosureHeader};
use crate::value::{POINTER_MASK, TAG_UNDEFINED};

const POINTER_TAG_BITS: u64 = 0x7FFD_0000_0000_0000;

thread_local! {
    /// Per-thread once-flag: ensures `media_callbacks_root_scanner` is
    /// only registered with the local GC scanner list once. ROOT_SCANNERS
    /// in `gc.rs` is thread-local, so each thread that interacts with
    /// `perry_media_*` registers on first use. Cheap (one branch + one
    /// thread-local read) on the steady state.
    static GC_SCANNER_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Register `media_callbacks_root_scanner` with the local thread's GC
/// once. Called from every `perry_media_*` FFI entry point so closure
/// pointers stored in `MEDIA_STATE_INBOX` survive between
/// `onStateChange` / `onTimeUpdate` invocations regardless of which
/// thread first interacts with the module.
fn ensure_gc_scanner_registered() {
    GC_SCANNER_REGISTERED.with(|flag| {
        if !flag.get() {
            crate::gc::gc_register_mutable_root_scanner_named(
                "media_callbacks",
                media_callbacks_root_scanner_mut,
            );
            flag.set(true);
        }
    });
}

// ---------------------------------------------------------------------------
// MediaState enum + string mapping
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MediaState {
    Idle,
    Loading,
    Ready,
    Playing,
    Paused,
    Ended,
    Error,
}

impl MediaState {
    pub fn as_str(self) -> &'static str {
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

    /// Map an ArkTS AVPlayer state string (`'idle'`, `'initialized'`,
    /// `'prepared'`, `'playing'`, `'paused'`, `'completed'`, `'released'`,
    /// `'stopped'`, `'error'`) to Perry's MediaState enum. Mapping mirrors
    /// the macOS / Android backends so the `state === 'ended'` check in
    /// user code works identically across platforms.
    pub fn from_avplayer_str(s: &str) -> MediaState {
        match s {
            "idle" => MediaState::Idle,
            "initialized" | "prepared" => MediaState::Ready,
            "playing" => MediaState::Playing,
            "paused" => MediaState::Paused,
            "completed" => MediaState::Ended,
            "stopped" => MediaState::Idle,
            "error" => MediaState::Error,
            // 'released' shouldn't be observable post-destroy. Map anything
            // else to Loading so the JS side sees a well-defined value.
            _ => MediaState::Loading,
        }
    }
}

// ---------------------------------------------------------------------------
// Drain-queue intent records
// ---------------------------------------------------------------------------

/// Player creation request — ArkTS allocates an AVPlayer, sets `url`, awaits
/// `prepare()`, and registers state/time observers.
#[derive(Debug, Clone)]
pub struct MediaCreateIntent {
    pub handle: i64,
    pub url: String,
}

/// Each variant is a control-plane verb against an existing player handle.
/// ArkTS dispatches `match` on the discriminant when draining.
#[derive(Debug, Clone)]
pub enum MediaCommand {
    Play { handle: i64 },
    Pause { handle: i64 },
    Stop { handle: i64 },
    Seek { handle: i64, seconds: f64 },
    SetVolume { handle: i64, volume: f64 },
    SetRate { handle: i64, rate: f64 },
    Destroy { handle: i64 },
}

/// Lock-screen / control-center metadata. AVSession on HarmonyOS — best
/// effort; ArkTS may no-op if AVSession plumbing isn't wired.
#[derive(Debug, Clone)]
pub struct MediaNowPlaying {
    pub handle: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_url: String,
}

// ---------------------------------------------------------------------------
// Static state
// ---------------------------------------------------------------------------

/// Monotonic 1-based handle allocator. AtomicI64 because `createPlayer`
/// can be called from worker threads (e.g. async fetch then create).
static HANDLE_SEQ: AtomicI64 = AtomicI64::new(0);

static MEDIA_CREATE_QUEUE: Mutex<Vec<MediaCreateIntent>> = Mutex::new(Vec::new());
static MEDIA_CONTROL_QUEUE: Mutex<Vec<MediaCommand>> = Mutex::new(Vec::new());
static MEDIA_NOW_PLAYING_QUEUE: Mutex<Vec<MediaNowPlaying>> = Mutex::new(Vec::new());

/// Per-handle observation pushed by ArkTS via `napi_push_media_state`.
/// Tuple is `(state, current_seconds, duration_seconds)`. Reads default to
/// `(Idle, 0.0, 0.0)` for never-observed handles — matches the spec'd
/// behavior of `getDuration` returning 0 on a live stream / loading state.
struct PlayerObservation {
    state: MediaState,
    current: f64,
    duration: f64,
    on_state_change: Option<f64>,
    on_time_update: Option<f64>,
    last_state_fired: Option<MediaState>,
}

impl Default for PlayerObservation {
    fn default() -> Self {
        Self {
            state: MediaState::Idle,
            current: 0.0,
            duration: 0.0,
            on_state_change: None,
            on_time_update: None,
            last_state_fired: None,
        }
    }
}

static MEDIA_STATE_INBOX: Mutex<Option<HashMap<i64, PlayerObservation>>> = Mutex::new(None);

fn with_inbox<R, F: FnOnce(&mut HashMap<i64, PlayerObservation>) -> R>(f: F) -> R {
    let mut guard = crate::gc::lock_gc_root_registry(&MEDIA_STATE_INBOX);
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

// ---------------------------------------------------------------------------
// String header decoding
// ---------------------------------------------------------------------------

/// Decode an `i64` raw `*StringHeader` (as the codegen passes for
/// `ArgKind::Str` thunks) to a Rust `String`. Mirrors the
/// `str_from_header` helpers in the per-platform `media_playback.rs`
/// files. Empty on null/zero pointer.
fn decode_string_header(ptr: i64) -> String {
    if ptr == 0 {
        return String::new();
    }
    unsafe {
        let header = ptr as *const crate::string::StringHeader;
        if header.is_null() {
            return String::new();
        }
        let blen = (*header).byte_len as usize;
        let data_ptr =
            (header as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, blen);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

// ---------------------------------------------------------------------------
// Public FFI surface — matches PERRY_MEDIA_TABLE in perry-dispatch
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn perry_media_create_player(url_ptr: i64) -> i64 {
    let url = decode_string_header(url_ptr);
    if url.is_empty() {
        return 0;
    }
    let handle = HANDLE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    with_inbox(|m| {
        m.entry(handle).or_default();
    });
    if let Ok(mut q) = MEDIA_CREATE_QUEUE.lock() {
        q.push(MediaCreateIntent { handle, url });
    }
    handle
}

#[no_mangle]
pub extern "C" fn perry_media_play(handle: f64) {
    enqueue_command(MediaCommand::Play {
        handle: handle as i64,
    });
}

#[no_mangle]
pub extern "C" fn perry_media_pause(handle: f64) {
    enqueue_command(MediaCommand::Pause {
        handle: handle as i64,
    });
}

#[no_mangle]
pub extern "C" fn perry_media_stop(handle: f64) {
    enqueue_command(MediaCommand::Stop {
        handle: handle as i64,
    });
}

#[no_mangle]
pub extern "C" fn perry_media_seek(handle: f64, seconds: f64) {
    enqueue_command(MediaCommand::Seek {
        handle: handle as i64,
        seconds,
    });
}

#[no_mangle]
pub extern "C" fn perry_media_set_volume(handle: f64, volume: f64) {
    enqueue_command(MediaCommand::SetVolume {
        handle: handle as i64,
        volume: volume.clamp(0.0, 1.0),
    });
}

#[no_mangle]
pub extern "C" fn perry_media_set_rate(handle: f64, rate: f64) {
    enqueue_command(MediaCommand::SetRate {
        handle: handle as i64,
        rate,
    });
}

#[no_mangle]
pub extern "C" fn perry_media_get_current_time(handle: f64) -> f64 {
    let h = handle as i64;
    with_inbox(|m| m.get(&h).map(|o| o.current).unwrap_or(0.0))
}

#[no_mangle]
pub extern "C" fn perry_media_get_duration(handle: f64) -> f64 {
    let h = handle as i64;
    with_inbox(|m| m.get(&h).map(|o| o.duration).unwrap_or(0.0))
}

#[no_mangle]
pub extern "C" fn perry_media_get_state(handle: f64) -> i64 {
    let h = handle as i64;
    let state = with_inbox(|m| m.get(&h).map(|o| o.state).unwrap_or(MediaState::Idle));
    let s = state.as_str();
    let header = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    header as i64
}

#[no_mangle]
pub extern "C" fn perry_media_is_playing(handle: f64) -> f64 {
    let h = handle as i64;
    let playing = with_inbox(|m| {
        matches!(
            m.get(&h).map(|o| o.state).unwrap_or(MediaState::Idle),
            MediaState::Playing
        )
    });
    if playing {
        1.0
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn perry_media_on_state_change(handle: f64, closure: f64) {
    ensure_gc_scanner_registered();
    let h = handle as i64;
    with_inbox(|m| {
        m.entry(h).or_default().on_state_change = Some(closure);
    });
}

#[no_mangle]
pub extern "C" fn perry_media_on_time_update(handle: f64, closure: f64) {
    ensure_gc_scanner_registered();
    let h = handle as i64;
    with_inbox(|m| {
        m.entry(h).or_default().on_time_update = Some(closure);
    });
}

#[no_mangle]
pub extern "C" fn perry_media_set_now_playing(
    handle: f64,
    title_ptr: i64,
    artist_ptr: i64,
    album_ptr: i64,
    artwork_ptr: i64,
) {
    let intent = MediaNowPlaying {
        handle: handle as i64,
        title: decode_string_header(title_ptr),
        artist: decode_string_header(artist_ptr),
        album: decode_string_header(album_ptr),
        artwork_url: decode_string_header(artwork_ptr),
    };
    if let Ok(mut q) = MEDIA_NOW_PLAYING_QUEUE.lock() {
        q.push(intent);
    }
}

#[no_mangle]
pub extern "C" fn perry_media_destroy(handle: f64) {
    let h = handle as i64;
    with_inbox(|m| {
        m.remove(&h);
    });
    enqueue_command(MediaCommand::Destroy { handle: h });
}

fn enqueue_command(cmd: MediaCommand) {
    if let Ok(mut q) = MEDIA_CONTROL_QUEUE.lock() {
        q.push(cmd);
    }
}

// ---------------------------------------------------------------------------
// NAPI drain accessors
// ---------------------------------------------------------------------------

/// Drain all pending create intents. Returns the queue contents and
/// clears it. Called from `ohos_napi::drain_media_create` on each timer
/// tick.
pub(crate) fn drain_create_intents() -> Vec<MediaCreateIntent> {
    MEDIA_CREATE_QUEUE
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

pub(crate) fn drain_control_commands() -> Vec<MediaCommand> {
    MEDIA_CONTROL_QUEUE
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

pub(crate) fn drain_now_playing_intents() -> Vec<MediaNowPlaying> {
    MEDIA_NOW_PLAYING_QUEUE
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// Push leftover create-intents back to the front of the queue. Used by
/// `napi_drain_media_create` to hand back exactly one intent per call
/// while preserving FIFO order across multiple ticks.
pub(crate) fn requeue_create_intents(mut tail: Vec<MediaCreateIntent>) {
    if tail.is_empty() {
        return;
    }
    if let Ok(mut q) = MEDIA_CREATE_QUEUE.lock() {
        // Anything queued while we were holding the popped intent goes
        // *after* the leftover tail — keep FIFO.
        tail.append(&mut *q);
        *q = tail;
    }
}

pub(crate) fn requeue_control_commands(mut tail: Vec<MediaCommand>) {
    if tail.is_empty() {
        return;
    }
    if let Ok(mut q) = MEDIA_CONTROL_QUEUE.lock() {
        tail.append(&mut *q);
        *q = tail;
    }
}

pub(crate) fn requeue_now_playing_intents(mut tail: Vec<MediaNowPlaying>) {
    if tail.is_empty() {
        return;
    }
    if let Ok(mut q) = MEDIA_NOW_PLAYING_QUEUE.lock() {
        tail.append(&mut *q);
        *q = tail;
    }
}

// ---------------------------------------------------------------------------
// State push from ArkTS — NAPI calls this when an AVPlayer event fires
// ---------------------------------------------------------------------------

/// Update the cached observation for `handle` and fire any registered
/// callbacks. State transitions fire `on_state_change(stateString)`;
/// every push fires `on_time_update(current, duration)` when the state
/// is Playing or Loading (matches the cross-platform contract — the
/// other backends only run the time-update callback while the timer is
/// in those states).
///
/// Called from `ohos_napi::napi_push_media_state` on the ArkTS UI
/// thread, which is also the only thread that can safely invoke a
/// Perry closure (closures capture this-thread arena pointers — see
/// the threading note in CLAUDE.md).
pub(crate) fn push_media_state(handle: i64, state: MediaState, current: f64, duration: f64) {
    let (on_state, on_time) = with_inbox(|m| {
        let obs = m.entry(handle).or_default();
        obs.current = current;
        obs.duration = duration;
        let prev = obs.state;
        obs.state = state;
        let state_changed = prev != state || obs.last_state_fired != Some(state);
        let on_state = if state_changed {
            obs.last_state_fired = Some(state);
            obs.on_state_change
        } else {
            None
        };
        let on_time = if matches!(state, MediaState::Playing | MediaState::Loading) {
            obs.on_time_update
        } else {
            None
        };
        (on_state, on_time)
    });

    if let Some(cb) = on_state {
        fire_state_callback(cb, state);
    }
    if let Some(cb) = on_time {
        fire_time_callback(cb, current, duration);
    }
}

fn unbox_closure(closure_d: f64) -> Option<*const ClosureHeader> {
    let bits = closure_d.to_bits();
    if (bits & !POINTER_MASK) != POINTER_TAG_BITS {
        return None;
    }
    let raw = (bits & POINTER_MASK) as *const ClosureHeader;
    if raw.is_null() {
        None
    } else {
        Some(raw)
    }
}

fn fire_state_callback(closure_d: f64, state: MediaState) {
    let Some(closure) = unbox_closure(closure_d) else {
        return;
    };
    let s = state.as_str();
    let header = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    if header.is_null() {
        let _ = js_closure_call1(closure, f64::from_bits(TAG_UNDEFINED));
        return;
    }
    let str_d = crate::value::js_nanbox_string(header as i64);
    let _ = js_closure_call1(closure, str_d);
}

fn fire_time_callback(closure_d: f64, current: f64, duration: f64) {
    let Some(closure) = unbox_closure(closure_d) else {
        return;
    };
    let _ = js_closure_call2(closure, current, duration);
}

// ---------------------------------------------------------------------------
// GC root scanner — registered closures must survive between drain ticks
// ---------------------------------------------------------------------------

/// Mark every registered state-change / time-update closure as live.
/// Called from `gc_init` (gated on `ohos-napi`) so the generational
/// mark-sweep doesn't reclaim closure bodies between AVPlayer events.
pub fn media_callbacks_root_scanner(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    media_callbacks_root_scanner_mut(&mut visitor);
}

pub fn media_callbacks_root_scanner_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut guard = crate::gc::lock_gc_root_registry(&MEDIA_STATE_INBOX);
    if let Some(map) = guard.as_mut() {
        for obs in map.values_mut() {
            if let Some(c) = obs.on_state_change.as_mut() {
                visitor.visit_nanbox_f64_slot(c);
            }
            if let Some(c) = obs.on_time_update.as_mut() {
                visitor.visit_nanbox_f64_slot(c);
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn test_seed_media_callback_roots(
    handle: i64,
    on_state_change: f64,
    on_time_update: f64,
) {
    with_inbox(|m| {
        let obs = m.entry(handle).or_default();
        obs.on_state_change = Some(on_state_change);
        obs.on_time_update = Some(on_time_update);
    });
}

#[cfg(test)]
pub(crate) fn test_media_callback_roots(handle: i64) -> (u64, u64) {
    let guard = crate::gc::lock_gc_root_registry(&MEDIA_STATE_INBOX);
    let Some(map) = guard.as_ref() else {
        return (0, 0);
    };
    let Some(obs) = map.get(&handle) else {
        return (0, 0);
    };
    (
        obs.on_state_change.map(f64::to_bits).unwrap_or(0),
        obs.on_time_update.map(f64::to_bits).unwrap_or(0),
    )
}

#[cfg(test)]
pub(crate) fn test_with_media_callback_roots_locked<R>(f: impl FnOnce() -> R) -> R {
    let _inbox = crate::gc::lock_gc_root_registry(&MEDIA_STATE_INBOX);
    f()
}
