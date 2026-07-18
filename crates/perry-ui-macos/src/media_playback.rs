//! Streaming media playback (`perry/media`) — AVPlayer-backed.
//!
//! Implements the receiver-less FFI surface declared in
//! `crates/perry-dispatch/src/lib.rs::PERRY_MEDIA_TABLE` and called from
//! TypeScript via `import { createPlayer, play, … } from "perry/media"`.
//!
//! Architecture:
//!
//! - Each `createPlayer(url)` returns a 1-based handle. Player state lives
//!   in `PLAYERS` (a `Vec<Option<PlayerEntry>>`) keyed by handle - 1.
//! - Playback uses `AVPlayer` + `AVPlayerItem(URL:)`. AVPlayer handles
//!   network buffering, codec dispatch, and seek/rate control natively.
//! - State changes are surfaced by polling `timeControlStatus` +
//!   `currentItem.status` + an "ended" flag set from
//!   `AVPlayerItemDidPlayToEndTimeNotification`. A 10 Hz `NSTimer` drives
//!   both the state-change callback (on transition) and the time-update
//!   callback (every tick while playing/loading).
//! - Now Playing metadata uses `MPNowPlayingInfoCenter`. Lock-screen / Touch
//!   Bar / Siri Remote play/pause/skip routes through `MPRemoteCommandCenter`.

use crate::ffi::sel_registerName;
use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use std::cell::RefCell;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, a: f64, b: f64) -> f64;
    // Signature matches the rest of perry-ui-macos (see audio.rs:133,
    // lib.rs:113/118/1662). Runtime returns `*mut StringHeader` which is
    // i64-sized on Apple platforms.
    fn js_string_new_sso(data: *const u8, len: u32) -> f64;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;

    // ObjC runtime FFI — signatures match the existing ones in app.rs:738+
    // (single-source canonicalisation isn't worth chasing for a 6-decl
    // overlap that the linker resolves to the same symbols regardless).
    fn objc_allocateClassPair(
        superclass: *const std::ffi::c_void,
        name: *const i8,
        extra: usize,
    ) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;

    // class_addMethod takes a function pointer of arbitrary shape — every
    // ObjC method has a different signature (this+_cmd plus N args). app.rs
    // declares it strictly typed for its specific 2-arg method; we declare
    // a raw `*const c_void` variant under a different Rust name so the two
    // cohabit without a "previously declared with different signature"
    // error. Same C symbol, different Rust binding.
    #[link_name = "class_addMethod"]
    fn class_add_method_raw(
        cls: *mut std::ffi::c_void,
        sel: *const std::ffi::c_void,
        imp: *const std::ffi::c_void,
        types: *const i8,
    ) -> bool;
}

// ---------------------------------------------------------------------------
// Player state
// ---------------------------------------------------------------------------

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
    player: Retained<AnyObject>, // AVPlayer
    item: Retained<AnyObject>, // AVPlayerItem (kept retained — AVPlayer holds a weak ref via property)
    state: MediaState,
    /// Set on `play()` to disambiguate ready-and-paused (which is "ready"
    /// before first play) from explicitly-paused (after a play+pause).
    has_started: bool,
    /// Set by the AVPlayerItemDidPlayToEndTimeNotification observer.
    ended: AtomicBool,
    /// Latest known duration, cached so we don't query AVPlayer every tick.
    duration_seconds: f64,
    on_state_change: Option<f64>, // NaN-boxed closure pointer
    on_time_update: Option<f64>,
}

thread_local! {
    static PLAYERS: RefCell<Vec<Option<PlayerEntry>>> = const { RefCell::new(Vec::new()) };
    static POLL_TIMER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    static POLL_TIMER_TARGET: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    static POLL_CLASS_REGISTERED: RefCell<bool> = const { RefCell::new(false) };
    static REMOTE_COMMAND_CLASS_REGISTERED: RefCell<bool> = const { RefCell::new(false) };
    /// Single delegate target shared across all players for AVPlayerItem
    /// end-of-playback notifications.
    static END_OBSERVER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    static END_OBSERVER_REGISTERED: RefCell<bool> = const { RefCell::new(false) };
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

fn nsstring(s: &str) -> Retained<objc2_foundation::NSString> {
    objc2_foundation::NSString::from_str(s)
}

// ---------------------------------------------------------------------------
// Public FFI — called from `crates/perry-ui-macos/src/lib.rs` thunks
// ---------------------------------------------------------------------------

pub fn create_player(url_ptr: *const u8) -> i64 {
    let url_str = str_from_header(url_ptr);
    if url_str.is_empty() {
        return 0;
    }

    unsafe {
        // NSURL.URLWithString: — accepts http(s), file://, etc.
        let nsurl_cls = match AnyClass::get(c"NSURL") {
            Some(c) => c,
            None => return 0,
        };
        let url_ns = nsstring(url_str);
        let url: *mut AnyObject = msg_send![nsurl_cls, URLWithString: &*url_ns];
        if url.is_null() {
            return 0;
        }

        let item_cls = match AnyClass::get(c"AVPlayerItem") {
            Some(c) => c,
            None => return 0,
        };
        let item: Retained<AnyObject> = msg_send![item_cls, playerItemWithURL: url];

        let player_cls = match AnyClass::get(c"AVPlayer") {
            Some(c) => c,
            None => return 0,
        };
        let player: Retained<AnyObject> = msg_send![player_cls, playerWithPlayerItem: &*item];

        // Subscribe this item to the end-of-playback notification stream
        // (the observer is shared across all players — it dispatches to the
        // right entry by AVPlayerItem identity).
        register_end_observer(&item);

        let entry = PlayerEntry {
            player,
            item,
            state: MediaState::Loading,
            has_started: false,
            ended: AtomicBool::new(false),
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

        // Spin up the global poll timer if it isn't already running.
        ensure_poll_timer();

        handle
    }
}

pub fn play(handle: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            unsafe {
                let _: () = msg_send![&*entry.player, play];
            }
            entry.has_started = true;
        }
    });
}

pub fn pause(handle: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let players = p.borrow();
        if let Some(Some(entry)) = players.get(idx) {
            unsafe {
                let _: () = msg_send![&*entry.player, pause];
            }
        }
    });
}

pub fn stop(handle: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            unsafe {
                let _: () = msg_send![&*entry.player, pause];
                seek_to(&entry.player, 0.0);
            }
            entry.has_started = false;
        }
    });
}

pub fn seek(handle: f64, seconds: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let players = p.borrow();
        if let Some(Some(entry)) = players.get(idx) {
            unsafe {
                seek_to(&entry.player, seconds);
            }
        }
    });
}

pub fn set_volume(handle: f64, volume: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    let v = volume.clamp(0.0, 1.0) as f32;
    PLAYERS.with(|p| {
        let players = p.borrow();
        if let Some(Some(entry)) = players.get(idx) {
            unsafe {
                let _: () = msg_send![&*entry.player, setVolume: v];
            }
        }
    });
}

pub fn set_rate(handle: f64, rate: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let players = p.borrow();
        if let Some(Some(entry)) = players.get(idx) {
            unsafe {
                let _: () = msg_send![&*entry.player, setRate: rate as f32];
            }
        }
    });
}

pub fn get_current_time(handle: f64) -> f64 {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return 0.0,
    };
    PLAYERS.with(|p| {
        let players = p.borrow();
        if let Some(Some(entry)) = players.get(idx) {
            unsafe { current_time_seconds(&entry.player) }
        } else {
            0.0
        }
    })
}

pub fn get_duration(handle: f64) -> f64 {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return 0.0,
    };
    PLAYERS.with(|p| {
        let players = p.borrow();
        if let Some(Some(entry)) = players.get(idx) {
            entry.duration_seconds.max(0.0)
        } else {
            0.0
        }
    })
}

pub fn get_state(handle: f64) -> i64 {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return empty_state_string(),
    };
    let state = PLAYERS.with(|p| {
        let players = p.borrow();
        players
            .get(idx)
            .and_then(|s| s.as_ref())
            .map(|e| e.state)
            .unwrap_or(MediaState::Idle)
    });
    let s = state.as_str();
    unsafe { js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64 }
}

pub fn is_playing(handle: f64) -> f64 {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return 0.0,
    };
    PLAYERS.with(|p| {
        let players = p.borrow();
        if let Some(Some(entry)) = players.get(idx) {
            if matches!(entry.state, MediaState::Playing) {
                1.0
            } else {
                0.0
            }
        } else {
            0.0
        }
    })
}

pub fn on_state_change(handle: f64, closure: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            entry.on_state_change = Some(closure);
        }
    });
}

pub fn on_time_update(handle: f64, closure: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(Some(entry)) = players.get_mut(idx) {
            entry.on_time_update = Some(closure);
        }
    });
}

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
    // The handle is currently advisory — MPNowPlayingInfoCenter is a
    // process-wide singleton, so the most recent setNowPlaying wins.
    // Holding the handle in the API keeps room for multi-player apps to
    // associate metadata with a specific player when we add a remote-
    // command dispatch table keyed by handle.
    let _ = handle;

    unsafe {
        let center_cls = match AnyClass::get(c"MPNowPlayingInfoCenter") {
            Some(c) => c,
            None => return,
        };
        let center: *mut AnyObject = msg_send![center_cls, defaultCenter];
        if center.is_null() {
            return;
        }

        let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
        let dict: Retained<AnyObject> = msg_send![dict_cls, new];

        if !title.is_empty() {
            // MPMediaItemPropertyTitle = "title"
            let _: () =
                msg_send![&*dict, setObject: &*nsstring(title), forKey: &*nsstring("title")];
        }
        if !artist.is_empty() {
            let _: () =
                msg_send![&*dict, setObject: &*nsstring(artist), forKey: &*nsstring("artist")];
        }
        if !album.is_empty() {
            let _: () =
                msg_send![&*dict, setObject: &*nsstring(album), forKey: &*nsstring("albumTitle")];
        }

        if !artwork.is_empty() {
            if let Some(image) = load_artwork(artwork) {
                if let Some(art) = make_artwork(&image) {
                    let _: () = msg_send![&*dict, setObject: &*art, forKey: &*nsstring("artwork")];
                }
            }
        }

        let _: () = msg_send![center, setNowPlayingInfo: &*dict];

        register_remote_commands(handle);
    }
}

pub fn destroy(handle: f64) {
    let idx = match handle_to_index(handle) {
        Some(i) => i,
        None => return,
    };
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(slot) = players.get_mut(idx) {
            if let Some(entry) = slot.take() {
                unsafe {
                    let _: () = msg_send![&*entry.player, pause];
                    // Tear down per-item end-of-playback observation by
                    // removing the AVPlayerItem from the global notification
                    // center's observer list for our shared target.
                    if let Some(observer) = END_OBSERVER.with(|o| o.borrow().clone()) {
                        let nc: *const AnyObject = msg_send![
                            AnyClass::get(c"NSNotificationCenter").unwrap(),
                            defaultCenter
                        ];
                        let _: () = msg_send![
                            nc,
                            removeObserver: &*observer,
                            name: &*nsstring("AVPlayerItemDidPlayToEndTimeNotification"),
                            object: &*entry.item
                        ];
                    }
                }
            }
        }
    });
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

fn empty_state_string() -> i64 {
    let s = "idle";
    unsafe { js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64 }
}

unsafe fn current_time_seconds(player: &AnyObject) -> f64 {
    // CMTime currentTime = [player currentTime];
    // CMTime is a {value: i64, timescale: i32, flags: u32, epoch: i64}.
    // CMTimeGetSeconds(time) handles the floating-point conversion;
    // calling it directly is cleaner than re-deriving the math here.
    extern "C" {
        fn CMTimeGetSeconds(time: CMTime) -> f64;
    }
    let time: CMTime = msg_send![player, currentTime];
    let secs = CMTimeGetSeconds(time);
    if secs.is_nan() || secs.is_infinite() {
        0.0
    } else {
        secs
    }
}

unsafe fn item_duration_seconds(item: &AnyObject) -> f64 {
    extern "C" {
        fn CMTimeGetSeconds(time: CMTime) -> f64;
    }
    let time: CMTime = msg_send![item, duration];
    let secs = CMTimeGetSeconds(time);
    if secs.is_nan() || secs.is_infinite() {
        0.0
    } else {
        secs
    }
}

unsafe fn seek_to(player: &AnyObject, seconds: f64) {
    extern "C" {
        fn CMTimeMakeWithSeconds(seconds: f64, preferred_timescale: i32) -> CMTime;
    }
    let target = CMTimeMakeWithSeconds(seconds, 600);
    let _: () = msg_send![player, seekToTime: target];
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

// objc2's `msg_send!` requires the return / arg type to implement `Encode`
// so it can synthesize the right ObjC type signature. CMTime is Apple's
// canonical "time as a rational number" struct — the stable encoding
// `{?=qiIq}` (struct, value=i64, timescale=i32, flags=u32, epoch=i64)
// matches `<CoreMedia/CMTime.h>`'s layout exactly.
unsafe impl objc2::Encode for CMTime {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
        "?",
        &[
            objc2::Encoding::LongLong, // value: i64
            objc2::Encoding::Int,      // timescale: i32
            objc2::Encoding::UInt,     // flags: u32
            objc2::Encoding::LongLong, // epoch: i64
        ],
    );
}

unsafe fn load_artwork(path_or_url: &str) -> Option<Retained<AnyObject>> {
    // NSImage(contentsOfFile:) for local paths; NSImage(contentsOfURL:) for remote.
    let image_cls = AnyClass::get(c"NSImage")?;
    if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        let nsurl_cls = AnyClass::get(c"NSURL")?;
        let url_ns = nsstring(path_or_url);
        let url: *mut AnyObject = msg_send![nsurl_cls, URLWithString: &*url_ns];
        if url.is_null() {
            return None;
        }
        let alloc: *mut AnyObject = msg_send![image_cls, alloc];
        let img: *mut AnyObject = msg_send![alloc, initWithContentsOfURL: url];
        if img.is_null() {
            None
        } else {
            Retained::from_raw(img)
        }
    } else {
        let stripped = path_or_url.strip_prefix("file://").unwrap_or(path_or_url);
        let path_ns = nsstring(stripped);
        let alloc: *mut AnyObject = msg_send![image_cls, alloc];
        let img: *mut AnyObject = msg_send![alloc, initWithContentsOfFile: &*path_ns];
        if img.is_null() {
            None
        } else {
            Retained::from_raw(img)
        }
    }
}

unsafe fn make_artwork(image: &AnyObject) -> Option<Retained<AnyObject>> {
    // MPMediaItemArtwork — initWithBoundsSize:requestHandler: is the modern
    // initializer (iOS 10+/macOS 10.12.2+). The handler is called with the
    // requested size; we just return our pre-loaded NSImage scaled by AppKit
    // automatically.
    let cls = AnyClass::get(c"MPMediaItemArtwork")?;
    let alloc: *mut AnyObject = msg_send![cls, alloc];
    // Image-only convenience initializer (deprecated but still works on
    // macOS): fall back to it when the block-based init is awkward from
    // Rust without a stable block-binding crate.
    let art: *mut AnyObject = msg_send![alloc, initWithImage: image];
    if art.is_null() {
        None
    } else {
        Retained::from_raw(art)
    }
}

// ---------------------------------------------------------------------------
// Poll timer (10 Hz state + time-update tick)
// ---------------------------------------------------------------------------

unsafe extern "C" fn poll_tick(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _timer: *mut AnyObject,
) {
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

            // Refresh cached duration whenever the item is ready (it can
            // become known on the second or third tick after createPlayer).
            if entry.duration_seconds == 0.0 {
                let d = item_duration_seconds(&entry.item);
                if d > 0.0 {
                    entry.duration_seconds = d;
                }
            }

            // Snapshot the closures we want to fire so we can release the
            // PLAYERS borrow before invoking JS — the closure itself may
            // call back into perry/media.
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
            let cur = current_time_seconds(&entry.player);
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
    // Belt-and-braces "ended" detection (issue #351, comment from acroyear).
    // The native notification (AVPlayerItemDidPlayToEndTimeNotification) sets
    // `entry.ended` and is the primary signal; a `currentTime ≈ duration`
    // fallback catches the cases where the notification fails to fire. The
    // same belt-and-braces is applied on every backend (Android, GStreamer,
    // Media Foundation) for parity and resilience.
    if entry.ended.load(Ordering::Relaxed) {
        return MediaState::Ended;
    }
    unsafe {
        let item_status: i64 = msg_send![&*entry.item, status];
        // AVPlayerItemStatusFailed = 2
        if item_status == 2 {
            return MediaState::Error;
        }
        // AVPlayerStatus.unknown = 0, .readyToPlay = 1, .failed = 2
        let player_status: i64 = msg_send![&*entry.player, status];
        if player_status == 2 {
            return MediaState::Error;
        }

        // Time-vs-duration fallback for end-of-stream detection. Engaged only
        // once playback has actually started AND duration is known (live
        // streams report duration as +inf, which we sanitise to 0.0 in
        // `item_duration_seconds`). 0.25s window is roughly 2-3 poll ticks
        // of headroom — small enough to feel immediate, wide enough to avoid
        // spurious flips on lossy seek.
        if entry.has_started && entry.duration_seconds > 0.25 {
            let cur = current_time_seconds(&entry.player);
            if cur >= entry.duration_seconds - 0.25 {
                entry.ended.store(true, Ordering::Relaxed);
                return MediaState::Ended;
            }
        }

        // timeControlStatus is the canonical source of truth for play/pause/buffering
        // (iOS 10+/macOS 10.12+).
        // .paused = 0, .waitingToPlayAtSpecifiedRate = 1, .playing = 2
        let tcs: i64 = msg_send![&*entry.player, timeControlStatus];
        match tcs {
            1 => MediaState::Loading,
            2 => MediaState::Playing,
            _ => {
                if !entry.has_started {
                    if player_status == 1 {
                        MediaState::Ready
                    } else {
                        MediaState::Loading
                    }
                } else {
                    MediaState::Paused
                }
            }
        }
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

fn register_poll_class() {
    POLL_CLASS_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;
        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryMediaPollTarget".as_ptr(), 0);
            if cls.is_null() {
                return;
            }
            let sel = sel_registerName(c"pollTick:".as_ptr());
            class_add_method_raw(
                cls,
                sel,
                poll_tick as *const std::ffi::c_void,
                c"v@:@".as_ptr(),
            );
            objc_registerClassPair(cls);
        }
    });
}

fn ensure_poll_timer() {
    if POLL_TIMER.with(|t| t.borrow().is_some()) {
        return;
    }
    register_poll_class();
    unsafe {
        let target_cls = match AnyClass::get(c"PerryMediaPollTarget") {
            Some(c) => c,
            None => return,
        };
        let target: Retained<AnyObject> = msg_send![target_cls, new];
        let sel = Sel::register(c"pollTick:");
        let timer: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.1f64,
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ];
        POLL_TIMER.with(|t| *t.borrow_mut() = Some(timer));
        POLL_TIMER_TARGET.with(|t| *t.borrow_mut() = Some(target));
    }
}

// ---------------------------------------------------------------------------
// AVPlayerItem end-of-playback observer
// ---------------------------------------------------------------------------

unsafe extern "C" fn handle_did_play_to_end(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    notification: *mut AnyObject,
) {
    if notification.is_null() {
        return;
    }
    let item: *mut AnyObject = msg_send![notification, object];
    if item.is_null() {
        return;
    }
    PLAYERS.with(|p| {
        let players = p.borrow();
        for slot in players.iter() {
            if let Some(entry) = slot.as_ref() {
                if &*entry.item as *const AnyObject == item as *const AnyObject {
                    entry.ended.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    });
}

fn ensure_end_observer_class() {
    END_OBSERVER_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;
        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryMediaEndObserver".as_ptr(), 0);
            if cls.is_null() {
                return;
            }
            let sel = sel_registerName(c"didPlayToEnd:".as_ptr());
            class_add_method_raw(
                cls,
                sel,
                handle_did_play_to_end as *const std::ffi::c_void,
                c"v@:@".as_ptr(),
            );
            objc_registerClassPair(cls);
        }
    });
}

fn register_end_observer(item: &AnyObject) {
    ensure_end_observer_class();
    unsafe {
        let observer = END_OBSERVER.with(|o| {
            let mut borrow = o.borrow_mut();
            if let Some(existing) = borrow.as_ref() {
                Some(existing.clone())
            } else {
                let cls = AnyClass::get(c"PerryMediaEndObserver")?;
                let new_obs: Retained<AnyObject> = msg_send![cls, new];
                *borrow = Some(new_obs.clone());
                Some(new_obs)
            }
        });
        let observer = match observer {
            Some(o) => o,
            None => return,
        };

        let nc: *const AnyObject = msg_send![
            AnyClass::get(c"NSNotificationCenter").unwrap(),
            defaultCenter
        ];
        let sel = Sel::register(c"didPlayToEnd:");
        let _: () = msg_send![
            nc,
            addObserver: &*observer,
            selector: sel,
            name: &*nsstring("AVPlayerItemDidPlayToEndTimeNotification"),
            object: item
        ];
    }
}

// ---------------------------------------------------------------------------
// Remote command center (lock screen / Touch Bar / Now Playing controls)
// ---------------------------------------------------------------------------

/// Remote-command handlers route to the **first** live player. A multi-
/// player app would need an explicit "active player" handle to disambiguate;
/// for the common single-player Subsonic / podcast use case the first-live
/// strategy matches user expectation (the player whose metadata is in the
/// Now Playing center is the one taking commands).
fn first_active_player_mut<F: FnOnce(&mut PlayerEntry)>(f: F) {
    PLAYERS.with(|p| {
        let mut players = p.borrow_mut();
        if let Some(entry) = players.iter_mut().filter_map(|s| s.as_mut()).next() {
            f(entry);
        }
    });
}

unsafe extern "C" fn remote_play_handler(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _event: *mut AnyObject,
) -> i64 {
    first_active_player_mut(|entry| {
        let _: () = msg_send![&*entry.player, play];
        entry.has_started = true;
    });
    0 // MPRemoteCommandHandlerStatusSuccess
}

unsafe extern "C" fn remote_pause_handler(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _event: *mut AnyObject,
) -> i64 {
    first_active_player_mut(|entry| {
        let _: () = msg_send![&*entry.player, pause];
    });
    0
}

unsafe extern "C" fn remote_toggle_handler(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _event: *mut AnyObject,
) -> i64 {
    first_active_player_mut(|entry| {
        let tcs: i64 = msg_send![&*entry.player, timeControlStatus];
        if tcs == 2 {
            let _: () = msg_send![&*entry.player, pause];
        } else {
            let _: () = msg_send![&*entry.player, play];
            entry.has_started = true;
        }
    });
    0
}

fn register_remote_commands(_handle: f64) {
    REMOTE_COMMAND_CLASS_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;
        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls =
                objc_allocateClassPair(superclass, c"PerryMediaRemoteCommandTarget".as_ptr(), 0);
            if cls.is_null() {
                return;
            }
            for (name, imp) in &[
                ("playEvent:", remote_play_handler as *const std::ffi::c_void),
                (
                    "pauseEvent:",
                    remote_pause_handler as *const std::ffi::c_void,
                ),
                (
                    "toggleEvent:",
                    remote_toggle_handler as *const std::ffi::c_void,
                ),
            ] {
                let cs = CString::new(*name).unwrap();
                let sel = sel_registerName(cs.as_ptr());
                // Type encoding: q@:@ — i64 ret, self, _cmd, NSObject (event)
                class_add_method_raw(cls, sel, *imp, c"q@:@".as_ptr());
            }
            objc_registerClassPair(cls);

            let target_cls = match AnyClass::get(c"PerryMediaRemoteCommandTarget") {
                Some(c) => c,
                None => return,
            };
            let target: Retained<AnyObject> = msg_send![target_cls, new];

            let cmd_center_cls = match AnyClass::get(c"MPRemoteCommandCenter") {
                Some(c) => c,
                None => return,
            };
            let cmd_center: *mut AnyObject = msg_send![cmd_center_cls, sharedCommandCenter];
            if cmd_center.is_null() {
                return;
            }

            // Bind play / pause / togglePlayPause commands. The
            // MPRemoteCommandCenter properties are zero-arg accessors, so
            // a plain `msg_send!` returns the per-command target collection
            // directly. `addTarget:action:` returns an opaque token which
            // we ignore — removeTarget would need it on cleanup, but the
            // command center is process-singleton so we leak by design.
            let play_cmd: *mut AnyObject = msg_send![cmd_center, playCommand];
            if !play_cmd.is_null() {
                let _: () = msg_send![play_cmd, setEnabled: true];
                let action = Sel::register(c"playEvent:");
                let _: *mut AnyObject = msg_send![play_cmd, addTarget: &*target, action: action];
            }
            let pause_cmd: *mut AnyObject = msg_send![cmd_center, pauseCommand];
            if !pause_cmd.is_null() {
                let _: () = msg_send![pause_cmd, setEnabled: true];
                let action = Sel::register(c"pauseEvent:");
                let _: *mut AnyObject = msg_send![pause_cmd, addTarget: &*target, action: action];
            }
            let toggle_cmd: *mut AnyObject = msg_send![cmd_center, togglePlayPauseCommand];
            if !toggle_cmd.is_null() {
                let _: () = msg_send![toggle_cmd, setEnabled: true];
                let action = Sel::register(c"toggleEvent:");
                let _: *mut AnyObject = msg_send![toggle_cmd, addTarget: &*target, action: action];
            }

            // Keep target alive; MPRemoteCommandCenter holds a weak ref.
            std::mem::forget(target);
        }
    });
}
