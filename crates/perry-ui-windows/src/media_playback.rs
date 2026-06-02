//! Windows streaming media playback (`perry/media`) — `Windows.Media.Playback.MediaPlayer`
//! via the `windows` crate.
//!
//! WinRT MediaPlayer is the modern replacement for the older Media
//! Foundation IMFMediaEngine surface. It handles HTTP/HTTPS streaming
//! URLs, codec dispatch, and SystemMediaTransportControls integration
//! (lock-screen) natively. Same handle-based pattern as the AVPlayer
//! macOS impl. State is polled at 10 Hz (matches macOS / Android /
//! GStreamer). EOS detection uses both `MediaEnded` event AND a
//! `position ≥ duration - 0.25s` fallback (issue #351 acroyear comment).
//!
//! Thread model: WinRT is COM-based. We MUST initialise apartment-
//! threaded COM (`RoInitialize`) on the calling thread, but can call
//! into MediaPlayer from any thread once initialised. The poll thread
//! does its own RoInitialize.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use windows::core::HSTRING;
use windows::Foundation::{TimeSpan, Uri};
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlaybackState, MediaPlayer};
use windows::Media::{MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControlsButton};
use windows::Storage::Streams::RandomAccessStreamReference;

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
    player: MediaPlayer,
    state: MediaState,
    has_started: bool,
    /// Set by the MediaEnded event handler OR by the position-vs-duration
    /// fallback (belt-and-braces per acroyear's #351 comment).
    ended: Arc<AtomicBool>,
    /// Set by the MediaFailed event handler.
    error: Arc<AtomicBool>,
    duration_seconds: f64,
    on_state_change: Option<f64>,
    on_time_update: Option<f64>,
    /// Last `MediaPlaybackStatus` we pushed to SMTC. Avoids redundant
    /// vtable calls when the derived state hasn't changed buckets.
    smtc_installed: bool,
    last_smtc_status: Option<MediaPlaybackStatus>,
}

thread_local! {
    static PLAYERS: RefCell<Vec<Option<PlayerEntry>>> = const { RefCell::new(Vec::new()) };
    /// Tick counter — `pump_tick()` fires from the message loop after
    /// every `GetMessageW` / `PeekMessageW` round (typically hundreds of
    /// times per second). Throttled to a 100 ms cadence (~10 Hz) so
    /// `onTimeUpdate` doesn't flood the JS callback queue.
    static PUMP_LAST_TICK_MS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
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

    let uri = match Uri::CreateUri(&HSTRING::from(url)) {
        Ok(u) => u,
        Err(_) => return 0,
    };
    let source = match MediaSource::CreateFromUri(&uri) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let player = match MediaPlayer::new() {
        Ok(p) => p,
        Err(_) => return 0,
    };
    if player.SetSource(&source).is_err() {
        return 0;
    }

    let ended = Arc::new(AtomicBool::new(false));
    let error = Arc::new(AtomicBool::new(false));
    install_event_handlers(&player, Arc::clone(&ended), Arc::clone(&error));

    let entry = PlayerEntry {
        player,
        state: MediaState::Loading,
        has_started: false,
        ended,
        error,
        duration_seconds: 0.0,
        on_state_change: None,
        on_time_update: None,
        smtc_installed: false,
        last_smtc_status: None,
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

    handle
}

pub fn play(handle: f64) {
    with_entry_mut(handle, |entry| {
        let _ = entry.player.Play();
        entry.has_started = true;
    });
}

pub fn pause(handle: f64) {
    with_entry(handle, |entry| {
        let _ = entry.player.Pause();
    });
}

pub fn stop(handle: f64) {
    with_entry_mut(handle, |entry| {
        let _ = entry.player.Pause();
        if let Ok(session) = entry.player.PlaybackSession() {
            let _ = session.SetPosition(TimeSpan { Duration: 0 });
        }
        entry.has_started = false;
    });
}

pub fn seek(handle: f64, seconds: f64) {
    with_entry(handle, |entry| {
        if let Ok(session) = entry.player.PlaybackSession() {
            let ticks = (seconds.max(0.0) * 10_000_000.0) as i64;
            let _ = session.SetPosition(TimeSpan { Duration: ticks });
        }
    });
}

pub fn set_volume(handle: f64, volume: f64) {
    with_entry(handle, |entry| {
        let _ = entry.player.SetVolume(volume.clamp(0.0, 1.0));
    });
}

pub fn set_rate(handle: f64, rate: f64) {
    with_entry(handle, |entry| {
        if let Ok(session) = entry.player.PlaybackSession() {
            let _ = session.SetPlaybackRate(rate);
        }
    });
}

pub fn get_current_time(handle: f64) -> f64 {
    with_entry(handle, |entry| {
        entry
            .player
            .PlaybackSession()
            .and_then(|s| s.Position())
            .map(|t| t.Duration as f64 / 10_000_000.0)
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

/// Wires `MediaPlayer.SystemMediaTransportControls` so the metadata shows
/// up on the Windows volume HUD media tile, the Edge / Chromium Now
/// Playing widget, and Bluetooth headphone media keys (#367).
///
/// `artworkUrl` accepts `https://` URLs (the common case — fed straight
/// to `RandomAccessStreamReference::CreateFromUri`). `file://` paths are
/// **not** supported in v1 — `StorageFile::GetFileFromPathAsync` is
/// genuinely asynchronous and the synchronous-blocking-on-`IAsyncOperation`
/// dance has its own gotchas in non-MTA threads. Pass an `https://` URL
/// or `""` (empty string skips artwork). Tracked as a follow-up.
pub fn set_now_playing(
    handle: f64,
    title_ptr: *const u8,
    artist_ptr: *const u8,
    album_ptr: *const u8,
    artwork_ptr: *const u8,
) {
    let title = str_from_header(title_ptr);
    let artist = str_from_header(artist_ptr);
    let album = str_from_header(album_ptr);
    let artwork = str_from_header(artwork_ptr);

    with_entry_mut(handle, |entry| {
        let smtc = match entry.player.SystemMediaTransportControls() {
            Ok(s) => s,
            Err(_) => return,
        };

        // Enable buttons. Idempotent — Windows ignores repeat sets.
        let _ = smtc.SetIsPlayEnabled(true);
        let _ = smtc.SetIsPauseEnabled(true);
        let _ = smtc.SetIsStopEnabled(true);
        let _ = smtc.SetIsNextEnabled(true);
        let _ = smtc.SetIsPreviousEnabled(true);

        if let Ok(updater) = smtc.DisplayUpdater() {
            let _ = updater.SetType(MediaPlaybackType::Music);
            if let Ok(music) = updater.MusicProperties() {
                if !title.is_empty() {
                    let _ = music.SetTitle(&HSTRING::from(title));
                }
                if !artist.is_empty() {
                    let _ = music.SetArtist(&HSTRING::from(artist));
                }
                if !album.is_empty() {
                    let _ = music.SetAlbumArtist(&HSTRING::from(album));
                }
            }

            if !artwork.is_empty() {
                if artwork.starts_with("https://") || artwork.starts_with("http://") {
                    if let Ok(uri) = Uri::CreateUri(&HSTRING::from(artwork)) {
                        if let Ok(stream) = RandomAccessStreamReference::CreateFromUri(&uri) {
                            let _ = updater.SetThumbnail(&stream);
                        }
                    }
                } else if artwork.starts_with("file://") {
                    // file:// requires StorageFile::GetFileFromPathAsync —
                    // skipped in v1 (see fn doc). Silently ignored.
                    eprintln!(
                        "perry/media: setNowPlaying file:// artwork not supported on Windows yet (#367 follow-up); use https://"
                    );
                }
            }
            let _ = updater.Update();
        }

        // Install ButtonPressed handler once per player. The handler
        // fires on a WinRT thread-pool worker — `play / pause / ...`
        // dispatch via `thread_local!` PLAYERS, which is empty on the
        // worker thread. So the handler enqueues into BUTTON_QUEUE
        // (cross-thread) and the main-thread `pump_tick` drains it.
        if !entry.smtc_installed {
            use windows::Foundation::TypedEventHandler;
            let player_handle = handle;
            let _ = smtc.ButtonPressed(&TypedEventHandler::new(move |_, args| {
                if let Some(args) = args {
                    let args: &windows::Media::SystemMediaTransportControlsButtonPressedEventArgs =
                        args;
                    if let Ok(button) = args.Button() {
                        enqueue_button(player_handle, button);
                    }
                }
                Ok(())
            }));
            entry.smtc_installed = true;
        }

        // Initial status push so the system UI doesn't open with stale
        // "Stopped". Mirrors the state poller's mapping.
        let status = state_to_smtc_status(entry.state);
        if entry.last_smtc_status != Some(status) {
            let _ = smtc.SetPlaybackStatus(status);
            entry.last_smtc_status = Some(status);
        }
    });
}

fn state_to_smtc_status(state: MediaState) -> MediaPlaybackStatus {
    match state {
        MediaState::Playing => MediaPlaybackStatus::Playing,
        MediaState::Paused | MediaState::Ready => MediaPlaybackStatus::Paused,
        MediaState::Ended => MediaPlaybackStatus::Stopped,
        MediaState::Idle | MediaState::Loading | MediaState::Error => MediaPlaybackStatus::Closed,
    }
}

// SMTC ButtonPressed fires on a WinRT thread-pool worker — drain to the
// main thread via `pump_tick`. `Mutex<Vec>` is fine; queue is tiny and
// only contended on actual button presses.
fn button_queue() -> &'static Mutex<Vec<(f64, SystemMediaTransportControlsButton)>> {
    static Q: OnceLock<Mutex<Vec<(f64, SystemMediaTransportControlsButton)>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

fn enqueue_button(handle: f64, button: SystemMediaTransportControlsButton) {
    if let Ok(mut q) = button_queue().lock() {
        q.push((handle, button));
    }
}

fn drain_buttons() {
    let drained: Vec<(f64, SystemMediaTransportControlsButton)> = match button_queue().lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for (handle, button) in drained {
        match button {
            SystemMediaTransportControlsButton::Play => play(handle),
            SystemMediaTransportControlsButton::Pause => pause(handle),
            SystemMediaTransportControlsButton::Stop => stop(handle),
            SystemMediaTransportControlsButton::FastForward => {
                let cur = get_current_time(handle);
                seek(handle, cur + 5.0);
            }
            SystemMediaTransportControlsButton::Rewind => {
                let cur = get_current_time(handle);
                seek(handle, (cur - 5.0).max(0.0));
            }
            // Next / Previous are queue-level concerns; v1 leaves them
            // as no-ops. Multi-track apps can wire their own queue logic
            // on top of `onStateChange`.
            _ => {}
        }
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
        let _ = entry.player.Close();
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
// Event handlers (MediaEnded / MediaFailed)
// ---------------------------------------------------------------------------

fn install_event_handlers(player: &MediaPlayer, ended: Arc<AtomicBool>, error: Arc<AtomicBool>) {
    use windows::Foundation::TypedEventHandler;

    let ended_clone = Arc::clone(&ended);
    let _ = player.MediaEnded(&TypedEventHandler::new(move |_, _| {
        ended_clone.store(true, Ordering::Relaxed);
        Ok(())
    }));
    let error_clone = Arc::clone(&error);
    let _ = player.MediaFailed(&TypedEventHandler::new(move |_, _| {
        error_clone.store(true, Ordering::Relaxed);
        Ok(())
    }));
}

// ---------------------------------------------------------------------------
// Pump tick — driven from `app.rs`'s `GetMessageW` / `PeekMessageW` loop
// (after each message dispatch). Internally throttled to ~10 Hz so
// `onTimeUpdate` doesn't flood the JS callback queue.
// ---------------------------------------------------------------------------

/// Wall-clock milliseconds since process start. Cheap and monotonic.
fn now_ms() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    start.elapsed().as_millis() as u64
}

/// Called from the main message loop. Cheap when there are no players
/// or when less than 100 ms has passed since the last actual tick.
pub fn pump_tick() {
    let now = now_ms();
    let should_run = PUMP_LAST_TICK_MS.with(|c| {
        let last = c.get();
        if now.saturating_sub(last) >= 100 {
            c.set(now);
            true
        } else {
            false
        }
    });
    if should_run {
        // Drain button presses queued from the WinRT worker thread first,
        // so a press that arrived since the last tick observes the right
        // player state when it dispatches.
        drain_buttons();
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

            // Refresh duration once playback session reports it.
            if entry.duration_seconds == 0.0 {
                if let Ok(session) = entry.player.PlaybackSession() {
                    if let Ok(dur) = session.NaturalDuration() {
                        let secs = dur.Duration as f64 / 10_000_000.0;
                        if secs > 0.0 {
                            entry.duration_seconds = secs;
                        }
                    }
                }
            }

            let new_state = derive_state(entry);
            let state_changed = new_state != entry.state;
            entry.state = new_state;

            // Push status to SMTC so the volume HUD / Edge Now Playing
            // tile reflect transitions (#367). Only after setNowPlaying
            // installed the handler — otherwise the SMTC isn't surfaced.
            if state_changed && entry.smtc_installed {
                let status = state_to_smtc_status(new_state);
                if entry.last_smtc_status != Some(status) {
                    if let Ok(smtc) = entry.player.SystemMediaTransportControls() {
                        let _ = smtc.SetPlaybackStatus(status);
                    }
                    entry.last_smtc_status = Some(status);
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
            let cur = entry
                .player
                .PlaybackSession()
                .and_then(|s| s.Position())
                .map(|t| t.Duration as f64 / 10_000_000.0)
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
            .player
            .PlaybackSession()
            .and_then(|s| s.Position())
            .map(|t| t.Duration as f64 / 10_000_000.0)
            .unwrap_or(0.0);
        if cur >= entry.duration_seconds - 0.25 {
            entry.ended.store(true, Ordering::Relaxed);
            return MediaState::Ended;
        }
    }

    let session_state = entry
        .player
        .PlaybackSession()
        .and_then(|s| s.PlaybackState())
        .unwrap_or(MediaPlaybackState::None);
    match session_state {
        MediaPlaybackState::Playing => MediaState::Playing,
        MediaPlaybackState::Paused => {
            if entry.has_started {
                MediaState::Paused
            } else {
                MediaState::Ready
            }
        }
        MediaPlaybackState::Buffering | MediaPlaybackState::Opening => MediaState::Loading,
        MediaPlaybackState::None => {
            if entry.has_started {
                MediaState::Paused
            } else {
                MediaState::Loading
            }
        }
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
