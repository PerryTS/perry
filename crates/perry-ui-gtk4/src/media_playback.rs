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
//!
//! Lock-screen / system media-key integration (#366) goes through MPRIS
//! — the canonical Linux desktop spec exposed over D-Bus as
//! `org.mpris.MediaPlayer2.Player`. The MPRIS server runs on a dedicated
//! thread with a tokio current-thread runtime owning the zbus connection;
//! D-Bus method calls (Play / Pause / Seek …) post commands into a
//! channel that the GLib poll tick drains, so all GStreamer pipeline
//! mutation stays on the main thread (pipelines are stored in a
//! thread_local). `set_now_playing` and state changes push the other
//! direction via a sync `properties_changed` call into the runtime.

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

    let uri = if url.contains("://") {
        url.to_string()
    } else {
        let path = if url.starts_with('/') {
            url.to_string()
        } else {
            std::env::current_dir()
                .map(|p| p.join(url).to_string_lossy().to_string())
                .unwrap_or_else(|_| url.to_string())
        };
        format!("file://{}", path)
    };

    let pipeline = match gstreamer::ElementFactory::make("playbin")
        .name("perry-media-playbin")
        .property("uri", uri.as_str())
        .build()
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[media] Failed to create playbin: {:?}", e);
            return 0;
        }
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
    title_ptr: *const u8,
    artist_ptr: *const u8,
    album_ptr: *const u8,
    artwork_ptr: *const u8,
) {
    let title = str_from_header(title_ptr).to_string();
    let artist = str_from_header(artist_ptr).to_string();
    let album = str_from_header(album_ptr).to_string();
    let artwork = str_from_header(artwork_ptr).to_string();
    #[cfg(target_os = "linux")]
    mpris::push_now_playing(title, artist, album, artwork);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (title, artist, album, artwork);
    }
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
                if let Some(dur) = entry.pipeline.query_duration::<gstreamer::ClockTime>() {
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

            // Always offer the current state to MPRIS (the impl dedupes
            // internally via LAST_STATUS). This makes sure the first
            // `set_now_playing` call after `play()` boots the D-Bus
            // server and immediately publishes the actual PlaybackStatus,
            // not just an old transition that fired before MPRIS was up.
            #[cfg(target_os = "linux")]
            mpris::push_playback_status(new_state);
            if let Some(cb) = on_state {
                fire_state_callback(cb, new_state);
            }
            if let Some(cb) = on_time {
                fire_time_callback(cb, cur, dur);
            }
        }
    });
    #[cfg(target_os = "linux")]
    mpris::drain_commands();
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

    let (_ret, current, _pending) = entry
        .pipeline
        .state(gstreamer::ClockTime::from_mseconds(50));
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

#[cfg(target_os = "linux")]
fn first_active_handle() -> Option<f64> {
    PLAYERS.with(|p| {
        let players = p.borrow();
        players
            .iter()
            .enumerate()
            .find(|(_, s)| s.is_some())
            .map(|(i, _)| (i + 1) as f64)
    })
}

#[cfg(target_os = "linux")]
fn current_position_seconds(handle: f64) -> f64 {
    with_entry(handle, |entry| {
        entry
            .pipeline
            .query_position::<gstreamer::ClockTime>()
            .map(|t| t.nseconds() as f64 / 1_000_000_000.0)
            .unwrap_or(0.0)
    })
    .unwrap_or(0.0)
}

#[cfg(target_os = "linux")]
mod mpris {
    //! D-Bus MPRIS server. Lazy-bootstrapped on the first
    //! `set_now_playing` call so apps that don't use Now Playing don't
    //! pay the zbus / tokio-runtime startup cost.
    //!
    //! Threading: the zbus connection lives on a dedicated tokio
    //! current-thread runtime. The `PlayerInterface` impl runs there
    //! and forwards method calls (Play / Pause / Seek …) into a
    //! `std::sync::mpsc` queue that `poll_tick` drains on the main
    //! GLib thread — that's where the GStreamer pipelines live
    //! (thread_local PLAYERS), so all pipeline mutation stays on
    //! the main thread.
    //!
    //! Property pushes (Metadata / PlaybackStatus) go the other way
    //! through a tokio mpsc channel: the main thread enqueues, the
    //! runtime drains and calls `Server::properties_changed` from
    //! within an async context.

    use super::{first_active_handle, MediaState};
    use mpris_server::{
        zbus::{fdo, Result as ZResult},
        LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property,
        RootInterface, Server, Time, TrackId, Volume,
    };
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{mpsc, Mutex, OnceLock};
    use tokio::sync::mpsc as tmpsc;

    /// D-Bus → main thread.
    enum Command {
        Play,
        Pause,
        PlayPause,
        Stop,
        /// offset in seconds (signed)
        SeekRelative(f64),
        /// absolute position in seconds
        SeekAbsolute(f64),
    }

    /// Main thread → tokio runtime.
    enum Update {
        Metadata(Metadata),
        Status(PlaybackStatus),
    }

    static CMD_TX: OnceLock<Mutex<mpsc::Sender<Command>>> = OnceLock::new();
    static CMD_RX: OnceLock<Mutex<mpsc::Receiver<Command>>> = OnceLock::new();
    static UPDATE_TX: OnceLock<tmpsc::UnboundedSender<Update>> = OnceLock::new();
    static INIT_FAILED: AtomicBool = AtomicBool::new(false);
    static TRACKID_COUNTER: AtomicU64 = AtomicU64::new(0);
    static LAST_STATUS: Mutex<Option<PlaybackStatus>> = Mutex::new(None);

    fn ensure_started() -> bool {
        if INIT_FAILED.load(Ordering::Relaxed) {
            return false;
        }
        if UPDATE_TX.get().is_some() {
            return true;
        }
        // Single-shot initializer — racing callers either both succeed
        // or both fail (the OnceLock writes are atomic).
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let _ = CMD_TX.set(Mutex::new(cmd_tx));
        let _ = CMD_RX.set(Mutex::new(cmd_rx));

        let (utx, urx) = tmpsc::unbounded_channel::<Update>();
        // The runtime owns urx; we share utx via OnceLock.
        if UPDATE_TX.set(utx).is_err() {
            return true; // another thread won the race
        }

        let pid = std::process::id();
        std::thread::Builder::new()
            .name("perry-mpris".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(_) => {
                        INIT_FAILED.store(true, Ordering::Relaxed);
                        return;
                    }
                };
                rt.block_on(async move {
                    run_server(pid, urx).await;
                });
            })
            .map(|_| true)
            .unwrap_or_else(|_| {
                INIT_FAILED.store(true, Ordering::Relaxed);
                false
            })
    }

    async fn run_server(pid: u32, mut urx: tmpsc::UnboundedReceiver<Update>) {
        // Bus name pattern `org.mpris.MediaPlayer2.<suffix>` — Server
        // prepends the prefix internally, so we only supply the
        // suffix (`perry-<pid>`).
        let suffix = format!("perry-{}", pid);
        let server = match Server::new(&suffix, PerryPlayer).await {
            Ok(s) => s,
            Err(_) => {
                INIT_FAILED.store(true, Ordering::Relaxed);
                return;
            }
        };
        while let Some(update) = urx.recv().await {
            let prop = match update {
                Update::Metadata(m) => Property::Metadata(m),
                Update::Status(s) => Property::PlaybackStatus(s),
            };
            let _ = server.properties_changed([prop]).await;
        }
    }

    pub fn push_now_playing(title: String, artist: String, album: String, artwork: String) {
        if !ensure_started() {
            return;
        }
        let trackid_str = format!(
            "/org/perry/track/{}",
            TRACKID_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let mut builder = Metadata::builder().title(title);
        if !artist.is_empty() {
            // Spec: xesam:artist is a string list even for a single artist.
            builder = builder.artist([artist]);
        }
        if !album.is_empty() {
            builder = builder.album(album);
        }
        if !artwork.is_empty() {
            builder = builder.art_url(artwork);
        }
        if let Ok(tid) = TrackId::try_from(trackid_str.as_str()) {
            builder = builder.trackid(tid);
        }
        let metadata = builder.build();
        if let Some(tx) = UPDATE_TX.get() {
            let _ = tx.send(Update::Metadata(metadata));
        }
    }

    pub fn push_playback_status(state: MediaState) {
        let status = match state {
            MediaState::Playing => PlaybackStatus::Playing,
            MediaState::Paused | MediaState::Ready => PlaybackStatus::Paused,
            MediaState::Idle | MediaState::Loading | MediaState::Ended | MediaState::Error => {
                PlaybackStatus::Stopped
            }
        };
        // Don't push if MPRIS hasn't been initialised yet — we only
        // bring up the D-Bus server on the first set_now_playing call,
        // and a state change before that means the app isn't using
        // Now Playing at all.
        if UPDATE_TX.get().is_none() {
            return;
        }
        // De-dupe identical status pushes; the GStreamer poll fires
        // every 100 ms and most ticks don't change state, but
        // derive_state() can still flip Loading↔Ready spuriously.
        let mut last = LAST_STATUS.lock().unwrap();
        if *last == Some(status) {
            return;
        }
        *last = Some(status);
        if let Some(tx) = UPDATE_TX.get() {
            let _ = tx.send(Update::Status(status));
        }
    }

    /// Drain any pending D-Bus commands and dispatch them to the
    /// first live player. Called from `poll_tick` on the main thread.
    pub fn drain_commands() {
        let rx = match CMD_RX.get() {
            Some(r) => r,
            None => return,
        };
        let rx = rx.lock().unwrap();
        while let Ok(cmd) = rx.try_recv() {
            let handle = match first_active_handle() {
                Some(h) => h,
                None => continue,
            };
            match cmd {
                Command::Play => super::play(handle),
                Command::Pause => super::pause(handle),
                Command::PlayPause => {
                    if super::is_playing(handle) >= 0.5 {
                        super::pause(handle);
                    } else {
                        super::play(handle);
                    }
                }
                Command::Stop => super::stop(handle),
                Command::SeekRelative(offset) => {
                    let cur = super::current_position_seconds(handle);
                    super::seek(handle, (cur + offset).max(0.0));
                }
                Command::SeekAbsolute(pos) => super::seek(handle, pos.max(0.0)),
            }
        }
    }

    fn send_cmd(cmd: Command) {
        if let Some(tx) = CMD_TX.get() {
            let _ = tx.lock().unwrap().send(cmd);
        }
    }

    struct PerryPlayer;

    impl RootInterface for PerryPlayer {
        async fn raise(&self) -> fdo::Result<()> {
            Ok(())
        }
        async fn quit(&self) -> fdo::Result<()> {
            Ok(())
        }
        async fn can_quit(&self) -> fdo::Result<bool> {
            Ok(false)
        }
        async fn fullscreen(&self) -> fdo::Result<bool> {
            Ok(false)
        }
        async fn set_fullscreen(&self, _fullscreen: bool) -> ZResult<()> {
            Ok(())
        }
        async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
            Ok(false)
        }
        async fn can_raise(&self) -> fdo::Result<bool> {
            Ok(false)
        }
        async fn has_track_list(&self) -> fdo::Result<bool> {
            Ok(false)
        }
        async fn identity(&self) -> fdo::Result<String> {
            Ok("Perry".into())
        }
        async fn desktop_entry(&self) -> fdo::Result<String> {
            Ok(String::new())
        }
        async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
            Ok(vec!["http".into(), "https".into(), "file".into()])
        }
        async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
            Ok(vec![
                "audio/mpeg".into(),
                "audio/aac".into(),
                "audio/ogg".into(),
                "audio/flac".into(),
                "audio/wav".into(),
                "audio/x-wav".into(),
            ])
        }
    }

    impl PlayerInterface for PerryPlayer {
        async fn next(&self) -> fdo::Result<()> {
            Ok(())
        }
        async fn previous(&self) -> fdo::Result<()> {
            Ok(())
        }
        async fn pause(&self) -> fdo::Result<()> {
            send_cmd(Command::Pause);
            Ok(())
        }
        async fn play_pause(&self) -> fdo::Result<()> {
            send_cmd(Command::PlayPause);
            Ok(())
        }
        async fn stop(&self) -> fdo::Result<()> {
            send_cmd(Command::Stop);
            Ok(())
        }
        async fn play(&self) -> fdo::Result<()> {
            send_cmd(Command::Play);
            Ok(())
        }
        async fn seek(&self, offset: Time) -> fdo::Result<()> {
            send_cmd(Command::SeekRelative(
                offset.as_micros() as f64 / 1_000_000.0,
            ));
            Ok(())
        }
        async fn set_position(&self, _track_id: TrackId, position: Time) -> fdo::Result<()> {
            send_cmd(Command::SeekAbsolute(
                position.as_micros() as f64 / 1_000_000.0,
            ));
            Ok(())
        }
        async fn open_uri(&self, _uri: String) -> fdo::Result<()> {
            Ok(())
        }
        async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
            Ok(LAST_STATUS
                .lock()
                .unwrap()
                .unwrap_or(PlaybackStatus::Stopped))
        }
        async fn loop_status(&self) -> fdo::Result<LoopStatus> {
            Ok(LoopStatus::None)
        }
        async fn set_loop_status(&self, _loop_status: LoopStatus) -> ZResult<()> {
            Ok(())
        }
        async fn rate(&self) -> fdo::Result<PlaybackRate> {
            Ok(1.0)
        }
        async fn set_rate(&self, _rate: PlaybackRate) -> ZResult<()> {
            Ok(())
        }
        async fn shuffle(&self) -> fdo::Result<bool> {
            Ok(false)
        }
        async fn set_shuffle(&self, _shuffle: bool) -> ZResult<()> {
            Ok(())
        }
        async fn metadata(&self) -> fdo::Result<Metadata> {
            Ok(Metadata::new())
        }
        async fn volume(&self) -> fdo::Result<Volume> {
            Ok(1.0)
        }
        async fn set_volume(&self, _volume: Volume) -> ZResult<()> {
            Ok(())
        }
        async fn position(&self) -> fdo::Result<Time> {
            Ok(Time::ZERO)
        }
        async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
            Ok(1.0)
        }
        async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
            Ok(1.0)
        }
        async fn can_go_next(&self) -> fdo::Result<bool> {
            Ok(false)
        }
        async fn can_go_previous(&self) -> fdo::Result<bool> {
            Ok(false)
        }
        async fn can_play(&self) -> fdo::Result<bool> {
            Ok(true)
        }
        async fn can_pause(&self) -> fdo::Result<bool> {
            Ok(true)
        }
        async fn can_seek(&self) -> fdo::Result<bool> {
            Ok(true)
        }
        async fn can_control(&self) -> fdo::Result<bool> {
            Ok(true)
        }
    }
}
