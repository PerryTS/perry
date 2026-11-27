//! `onFrame` driver: fires `js_frame_tick` on the display refresh (#15488).
//!
//! One frame source at a time, picked by [`select`]:
//!
//! - macOS 14+: a `CADisplayLink` from the main (else any visible) window,
//!   which follows the window across screens. A window that is not on any
//!   display gets no callbacks (AppKit `NSWindow.h`), so with no visible window
//!   the link comes from the main screen instead.
//! - macOS 13 and earlier: one `CVDisplayLink` ([`core_video`]), pointed at
//!   that window's screen.
//! - No screen at all: the timer pump ticks frames itself.
//!
//! Window and screen notifications re-run the selection. Every source delivers
//! in the default run-loop mode only, like the timer pump, so a frame callback
//! never re-enters JS from inside a modal or tracking loop. The source is
//! paused while no frame callback is pending; the timer pump resumes it.

mod core_video;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSDefaultRunLoopMode, NSObject, NSRunLoop, NSString};
use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

extern "C" {
    fn js_frame_tick(timestamp_ms: f64) -> i32;
    fn js_frame_has_pending() -> i32;
    fn js_performance_now() -> f64;
    fn js_promise_run_microtasks() -> i32;
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
    fn CGMainDisplayID() -> u32;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

/// Notifications after which another window or screen may be the right source.
/// An occlusion change is the only one posted when a window is ordered in or
/// out, and it is also posted on (de)miniaturizing.
const RESELECT_ON: [&str; 5] = [
    "NSWindowWillCloseNotification",
    "NSWindowDidChangeOcclusionStateNotification",
    "NSWindowDidBecomeMainNotification",
    "NSWindowDidChangeScreenNotification",
    "NSApplicationDidChangeScreenParametersNotification",
];

enum Source {
    /// A `CADisplayLink` on the main run loop, made by `owner`: a window, or
    /// the main screen when no window is on a display.
    Native {
        link: Retained<AnyObject>,
        owner: *const AnyObject,
    },
    CoreVideo(core_video::Link),
}

impl Source {
    /// Whether the source is now in the requested state.
    fn set_paused(&self, paused: bool) -> bool {
        match self {
            Source::Native { link, .. } => unsafe {
                let _: () = msg_send![&**link, setPaused: paused];
                true
            },
            Source::CoreVideo(link) => link.set_paused(paused),
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        match self {
            // Removes the link from the run loop, which releases it.
            Source::Native { link, .. } => unsafe {
                let _: () = msg_send![&**link, invalidate];
            },
            Source::CoreVideo(link) => {
                link.set_paused(true);
            }
        }
    }
}

thread_local! {
    static SOURCE: RefCell<Option<Source>> = const { RefCell::new(None) };
    static PAUSED: Cell<bool> = const { Cell::new(true) };
    /// The callback target shared by every source; lives for the process.
    static TARGET: Cell<*const AnyObject> = const { Cell::new(std::ptr::null()) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryFrameLinkTarget"]
    pub struct PerryFrameLinkTarget;

    impl PerryFrameLinkTarget {
        #[unsafe(method(frame:))]
        fn frame(&self, link: &AnyObject) {
            let timestamp: f64 = unsafe { msg_send![link, timestamp] };
            tick(timestamp);
        }

        #[unsafe(method(coreVideoFrame:))]
        fn core_video_frame(&self, _arg: Option<&AnyObject>) {
            if let Some(seconds) = core_video::take_queued_host_seconds() {
                tick(seconds);
            }
        }
    }
);

fn target() -> &'static AnyObject {
    TARGET.with(|cell| {
        if cell.get().is_null() {
            let target: Retained<PerryFrameLinkTarget> =
                unsafe { msg_send![PerryFrameLinkTarget::alloc(), init] };
            cell.set(Retained::into_raw(target) as *const AnyObject);
        }
        unsafe { &*cell.get() }
    })
}

fn host_ticks_to_seconds(ticks: u64) -> f64 {
    static TIMEBASE: OnceLock<MachTimebaseInfo> = OnceLock::new();
    let info = TIMEBASE.get_or_init(|| {
        let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
        unsafe { mach_timebase_info(&mut info) };
        info
    });
    ticks as f64 * info.numer as f64 / info.denom as f64 / 1e9
}

/// `performance.now()` minus the host clock, in milliseconds. Both count mach
/// time, so the offset is constant: it is sampled once, from the tightest of a
/// few bracketed reads, so a preemption between two reads cannot skew it.
fn performance_minus_host_ms() -> f64 {
    static OFFSET: OnceLock<f64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        (0..8)
            .map(|_| unsafe {
                let before = js_performance_now();
                let host = host_ticks_to_seconds(mach_absolute_time()) * 1000.0;
                let after = js_performance_now();
                (after - before, (before + after) / 2.0 - host)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(0.0, |(_, offset)| offset)
    })
}

/// Map a vsync's host time (seconds, the `CACurrentMediaTime` clock) onto the
/// `performance.now()` timeline: sub-millisecond, and comparable with it.
fn vsync_ms(host_seconds: f64) -> f64 {
    host_seconds * 1000.0 + performance_minus_host_ms()
}

fn tick(host_seconds: f64) {
    crate::catch_callback_panic(
        "onFrame",
        std::panic::AssertUnwindSafe(|| unsafe {
            js_frame_tick(vsync_ms(host_seconds));
            js_promise_run_microtasks();
        }),
    );
    if unsafe { js_frame_has_pending() } == 0 {
        set_paused(true);
    }
}

fn set_paused(paused: bool) {
    if PAUSED.with(|p| p.replace(paused)) == paused {
        return;
    }
    let applied = SOURCE.with(|slot| slot.borrow().as_ref().is_none_or(|s| s.set_paused(paused)));
    if !applied {
        // A source that cannot start is dropped, so the pump ticks frames.
        PAUSED.with(|p| p.set(true));
        let failed = SOURCE.with(|slot| slot.borrow_mut().take());
        drop(failed);
    }
}

/// The window to sync to: the main window, else any one on a display, skipping
/// `closing` (still on screen while its close notification is delivered). A
/// window covered by others still gets callbacks, so occlusion does not count.
fn frame_window(closing: *const AnyObject) -> Option<Retained<AnyObject>> {
    let eligible = |window: &AnyObject| unsafe {
        let visible: bool = msg_send![window, isVisible];
        let minimized: bool = msg_send![window, isMiniaturized];
        let screen: *mut AnyObject = msg_send![window, screen];
        !std::ptr::eq(window, closing) && visible && !minimized && !screen.is_null()
    };
    unsafe {
        let app: Retained<AnyObject> = msg_send![objc2::class!(NSApplication), sharedApplication];
        let main: Option<Retained<AnyObject>> = msg_send![&*app, mainWindow];
        if let Some(main) = main.filter(|w| eligible(w)) {
            return Some(main);
        }
        let windows: Retained<AnyObject> = msg_send![&*app, windows];
        let count: usize = msg_send![&*windows, count];
        (0..count)
            .map(|i| -> Retained<AnyObject> { msg_send![&*windows, objectAtIndex: i] })
            .find(|w| eligible(w))
    }
}

fn screen_display_id(window: &AnyObject) -> Option<u32> {
    unsafe {
        let screen: *mut AnyObject = msg_send![window, screen];
        if screen.is_null() {
            return None;
        }
        let description: *mut AnyObject = msg_send![screen, deviceDescription];
        let key = NSString::from_str("NSScreenNumber");
        let number: *mut AnyObject = msg_send![description, objectForKey: &*key];
        (!number.is_null()).then(|| msg_send![number, unsignedIntValue])
    }
}

fn has_native_links() -> bool {
    let sel = Sel::register(c"displayLinkWithTarget:selector:");
    unsafe { msg_send![objc2::class!(NSWindow), instancesRespondToSelector: sel] }
}

/// A `CADisplayLink` from `owner`, an `NSWindow` or `NSScreen`.
fn native_source(owner: &AnyObject) -> Source {
    unsafe {
        let link: Retained<AnyObject> = msg_send![
            owner,
            displayLinkWithTarget: target(),
            selector: objc2::sel!(frame:)
        ];
        let _: () = msg_send![&*link, setPaused: PAUSED.with(Cell::get)];
        let _: () = msg_send![
            &*link,
            addToRunLoop: &*NSRunLoop::mainRunLoop(),
            forMode: NSDefaultRunLoopMode
        ];
        Source::Native {
            link,
            owner: owner as *const AnyObject,
        }
    }
}

/// Point frames at the best available display, keeping the current source when
/// it already tracks the chosen window.
fn select(closing: *const AnyObject) {
    let window = frame_window(closing);
    let window = window.as_deref();
    if !has_native_links() {
        let display = window
            .and_then(screen_display_id)
            .unwrap_or_else(|| unsafe { CGMainDisplayID() });
        SOURCE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                // Without a link that starts, the pump ticks frames.
                *slot = core_video::Link::new(target())
                    .map(Source::CoreVideo)
                    .filter(|source| source.set_paused(PAUSED.with(Cell::get)));
            }
            if let Some(Source::CoreVideo(link)) = slot.as_ref() {
                link.set_display(display);
            }
        });
        return;
    }
    let screen: Option<Retained<AnyObject>> =
        unsafe { msg_send![objc2::class!(NSScreen), mainScreen] };
    let owner = window.or(screen.as_deref());
    let owner_ptr = owner.map_or(std::ptr::null(), |o| o as *const AnyObject);
    let unchanged = SOURCE.with(|slot| match slot.borrow().as_ref() {
        Some(Source::Native { owner, .. }) => *owner == owner_ptr,
        _ => owner_ptr.is_null(),
    });
    if unchanged {
        return;
    }
    let source = owner.map(native_source);
    // Drop the old source outside the borrow: `invalidate` releases objects.
    let old = SOURCE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), source));
    drop(old);
}

fn observe_windows_and_screens() {
    unsafe {
        let center: Retained<AnyObject> =
            msg_send![objc2::class!(NSNotificationCenter), defaultCenter];
        for name in RESELECT_ON {
            let is_close = name == "NSWindowWillCloseNotification";
            let block = block2::RcBlock::new(move |notification: *mut AnyObject| {
                let closing: *const AnyObject = if is_close && !notification.is_null() {
                    msg_send![notification, object]
                } else {
                    std::ptr::null()
                };
                crate::catch_callback_panic(
                    "onFrame source",
                    std::panic::AssertUnwindSafe(|| select(closing)),
                );
            });
            let name = NSString::from_str(name);
            let _: Retained<AnyObject> = msg_send![
                &*center,
                addObserverForName: &*name,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*block
            ];
        }
    }
}

/// Start driving frames from the display. Called once, from `app_run`, after
/// the app's windows are shown.
pub(crate) fn install() {
    observe_windows_and_screens();
    select(std::ptr::null());
    pump();
}

/// Called from the timer pump: wake the source when a frame callback is
/// pending, or tick frames directly when there is no source.
pub(crate) fn pump() {
    if SOURCE.with(|slot| slot.borrow().is_none()) {
        // The display-link clock, so a callback keeps one timeline when its
        // source goes away.
        unsafe { js_frame_tick(js_performance_now()) };
    } else if unsafe { js_frame_has_pending() } != 0 {
        set_paused(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vsync_timestamps_are_fractional_milliseconds_on_the_performance_now_clock() {
        let before = unsafe { js_performance_now() };
        let host_now = host_ticks_to_seconds(unsafe { mach_absolute_time() });
        let after = unsafe { js_performance_now() };
        let now = vsync_ms(host_now);
        assert!(
            before - 0.01 <= now && now <= after + 0.01,
            "{before} <= {now} <= {after}"
        );

        let frame = vsync_ms(host_now + 1.0 / 60.0) - vsync_ms(host_now);
        assert!(
            (frame - 1000.0 / 60.0).abs() < 0.01,
            "60 Hz frame gap {frame}"
        );
        assert_ne!(frame, frame.round());
    }
}
