//! GTK4 / Linux streaming media playback (`perry/media`) — GStreamer
//! `playbin` element.
//!
//! `playbin` is GStreamer's reference auto-pipeline: it picks the right
//! demuxer, decoder, and audio sink for the URI you hand it. Same
//! handle-based pattern as the AVPlayer macOS impl. State changes
//! and EOS arrive on the bus, which a glib::idle_add_local poller
//! drains every 100 ms (the same cadence as the macOS NSTimer poll).
//!
//! Belt-and-braces `ended` detection per acroyear's #351 comment:
//! both the GStreamer EOS message AND a `position ≥ duration - 0.25s`
//! fallback set the same `ended` flag, so the JS state-change callback
//! fires once per track even if EOS is dropped (rare, but cheap insurance).

use gstreamer::prelude::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, a: f64, b: f64) -> f64;
    fn js_string_from_bytes(data: *const u8, len: i32) -> i64;
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
    pipeline: gstreamer::Element,
    state: MediaState,
    has_started: bool,
    /// Set by the bus listener on EOS message OR by the
    /// position-vs-duration fallback in the polling tick.
    ended: Arc<AtomicBool>,
    /// Set by the bus listener on Error message.
    error: Arc<AtomicBool>,
    duration_seconds: f64,
    on_state_change: Option<f64>,
    on_time_update: Option<f64>,
}

thread_local! {
    static PLAYERS: RefCell<Vec<Option<PlayerEntry>>> = const { RefCell::new(Vec::new()) };
    static GST_INITIALIZED: RefCell<bool> = const { RefCell::new(false) };
    static POLL_TIMEOUT_INSTALLED: RefCell<bool> = const { RefCell::new(false) };
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

fn ensure_gst_init() {
    GST_INITIALIZED.with(|i| {
        if !*i.borrow() {
            // Idempotent — `gst::init` checks an internal "already
            // initialised" flag and returns Ok in that case.
            let _ = gstreamer::init();
            *i.borrow_mut() = true;
        }
    });
}

// ---------------------------------------------------------------------------
// Public FFI
// ---------------------------------------------------------------------------

pub fn create_player(url_ptr: *const u8) -> i64 {
    let url = str_from_header(url_ptr);
    if url.is_empty() {
        return 0;
    }
    ensure_gst_init();

    let pipeline = match gstreamer::ElementFactory::make("playbin")
        .name("perry-media-playbin")
        .property("uri", url)
        .build()
    {
        Ok(p) => p,
        Err(_) => return 0,
    };

    // Drive to PAUSED so the demuxer + decoder pick up duration metadata
    // before play() is ever called. PAUSED is the "ready to play" state
    // in GStreamer parlance.
    if pipeline.set_state(gstreamer::State::Paused).is_err() {
        return 0;
    }

    let ended = Arc::new(AtomicBool::new(false));
    let error = Arc::new(AtomicBool::new(false));
    install_bus_listener(&pipeline, Arc::clone(&ended), Arc::clone(&error));

    let entry = PlayerEntry {
        pipeline,
        state: MediaState::Loading,
        has_started: false,
        ended,
        error,
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

    ensure_poll_timeout();
    handle
}

pub fn play(handle: f64) {
    with_entry_mut(handle, |entry| {
        let _ = entry.pipeline.set_state(gstreamer::State::Playing);
        entry.has_started = true;
    });
}

pub fn pause(handle: f64) {
    with_entry_mut(handle, |entry| {
        let _ = entry.pipeline.set_state(gstreamer::State::Paused);
    });
}

pub fn stop(handle: f64) {
    with_entry_mut(handle, |entry| {
        let _ = entry.pipeline.set_state(gstreamer::State::Ready);
        entry.has_started = false;
    });
}

pub fn seek(handle: f64, seconds: f64) {
    with_entry(handle, |entry| {
        let pos = gstreamer::ClockTime::from_nseconds((seconds * 1_000_000_000.0).max(0.0) as u64);
        let _ = entry.pipeline.seek_simple(
            gstreamer::SeekFlags::FLUSH | gstreamer::SeekFlags::KEY_UNIT,
            pos,
        );
    });
}

pub fn set_volume(handle: f64, volume: f64) {
    with_entry(handle, |entry| {
        // playbin exposes a "volume" property as f64 in [0, 10].
        entry
            .pipeline
            .set_property("volume", volume.clamp(0.0, 1.0));
    });
}

pub fn set_rate(handle: f64, rate: f64) {
    with_entry(handle, |entry| {
        // GStreamer rate change goes through a seek with rate parameter.
        // Use the current position as the seek target so audio doesn't jump.
        let cur = entry
            .pipeline
            .query_position::<gstreamer::ClockTime>()
            .unwrap_or(gstreamer::ClockTime::ZERO);
        let _ = entry.pipeline.seek(
            rate,
            gstreamer::SeekFlags::FLUSH | gstreamer::SeekFlags::KEY_UNIT,
            gstreamer::SeekType::Set,
            cur,
            gstreamer::SeekType::None,
            gstreamer::ClockTime::NONE,
        );
    });
}

pub fn get_current_time(handle: f64) -> f64 {
    with_entry(handle, |entry| {
        entry
            .pipeline
            .query_position::<gstreamer::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1_000_000_000.0)
            .unwrap_or(0.0)
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
    // MPRIS is the canonical Linux lock-screen / media-key protocol.
    // It's a D-Bus interface (org.mpris.MediaPlayer2.Player) — we'd
    // expose ourselves as an MPRIS service so GNOME / KDE / playerctl
    // can issue play/pause/skip events. Tracked as a #351 follow-up;
    // the metadata is silently dropped here so callers don't have to
    // feature-detect.
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
        let _ = entry.pipeline.set_state(gstreamer::State::Null);
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

// ---------------------------------------------------------------------------
// Bus listener (EOS / error / state changes)
// ---------------------------------------------------------------------------

fn install_bus_listener(
    pipeline: &gstreamer::Element,
    ended: Arc<AtomicBool>,
    error: Arc<AtomicBool>,
) {
    let bus = match pipeline.bus() {
        Some(b) => b,
        None => return,
    };
    // `add_watch_local` calls the closure on the GLib main thread. We
    // forward EOS / error to the per-player atomic flags; the polling
    // tick reads those flags and produces the state callback.
    let _ = bus.add_watch_local(move |_bus, msg| {
        use gstreamer::MessageView;
        match msg.view() {
            MessageView::Eos(_) => ended.store(true, Ordering::Relaxed),
            MessageView::Error(_) => error.store(true, Ordering::Relaxed),
            _ => {}
        }
        gstreamer::glib::ControlFlow::Continue
    });
}

// ---------------------------------------------------------------------------
// Poll timeout (100 ms tick)
// ---------------------------------------------------------------------------

fn ensure_poll_timeout() {
    let already = POLL_TIMEOUT_INSTALLED.with(|s| *s.borrow());
    if already {
        return;
    }
    POLL_TIMEOUT_INSTALLED.with(|s| *s.borrow_mut() = true);

    // glib::timeout_add_local fires on the main loop, same thread as
    // the GTK widgets. PLAYERS is a thread_local so this is safe.
    gstreamer::glib::timeout_add_local(std::time::Duration::from_millis(100), || {
        poll_tick();
        gstreamer::glib::ControlFlow::Continue
    });
}

fn poll_tick() {
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        for slot in players.iter_mut() {
            let entry = match slot {
                Some(e) => e,
                None => continue,
            };

            // Refresh duration once GStreamer has it. query_duration
            // returns Some only after the demuxer has parsed enough
            // headers to know the total length; live streams stay None
            // forever (which leaves duration_seconds at 0.0 and disables
            // the time/duration `ended` fallback — correct).
            if entry.duration_seconds == 0.0 {
                if let Some(dur) =
                    entry.pipeline.query_duration::<gstreamer::ClockTime>()
                {
                    entry.duration_seconds = dur.nseconds() as f64 / 1_000_000_000.0;
                }
            }

            let new_state = derive_state(entry);
            let state_changed = new_state != entry.state;
            entry.state = new_state;

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
            let cur = entry
                .pipeline
                .query_position::<gstreamer::ClockTime>()
                .map(|t| t.nseconds() as f64 / 1_000_000_000.0)
                .unwrap_or(0.0);
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

fn derive_state(entry: &PlayerEntry) -> MediaState {
    if entry.error.load(Ordering::Relaxed) {
        return MediaState::Error;
    }
    if entry.ended.load(Ordering::Relaxed) {
        return MediaState::Ended;
    }
    // Belt-and-braces ended detection (issue #351 acroyear comment).
    if entry.has_started && entry.duration_seconds > 0.25 {
        let cur = entry
            .pipeline
            .query_position::<gstreamer::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1_000_000_000.0)
            .unwrap_or(0.0);
        if cur >= entry.duration_seconds - 0.25 {
            entry.ended.store(true, Ordering::Relaxed);
            return MediaState::Ended;
        }
    }

    let (_ret, current, _pending) = entry.pipeline.state(gstreamer::ClockTime::from_mseconds(50));
    match current {
        gstreamer::State::Playing => MediaState::Playing,
        gstreamer::State::Paused => {
            if entry.has_started {
                MediaState::Paused
            } else {
                MediaState::Ready
            }
        }
        gstreamer::State::Null | gstreamer::State::Ready => MediaState::Loading,
        _ => MediaState::Loading,
    }
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
