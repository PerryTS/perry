//! `CVDisplayLink` frame source for macOS 13 and earlier, where AppKit has no
//! display-link factory. The link calls back on its own high-priority thread;
//! each vsync is forwarded to the main thread in the default run-loop mode, and
//! vsyncs that arrive while one is still queued are coalesced into it.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::NSDefaultRunLoopMode;
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

type CVDisplayLinkRef = *mut c_void;

/// The leading fields of `CVTimeStamp` (CoreVideo/CVBase.h), read through the
/// pointer CoreVideo passes.
#[repr(C)]
struct CVTimeStamp {
    version: u32,
    video_time_scale: i32,
    video_time: i64,
    host_time: u64,
    rate_scalar: f64,
    video_refresh_period: i64,
}

type CVOutputCallback = extern "C" fn(
    CVDisplayLinkRef,
    *const CVTimeStamp,
    *const CVTimeStamp,
    u64,
    *mut u64,
    *mut c_void,
) -> i32;

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVDisplayLinkCreateWithActiveCGDisplays(out: *mut CVDisplayLinkRef) -> i32;
    fn CVDisplayLinkSetCurrentCGDisplay(link: CVDisplayLinkRef, display: u32) -> i32;
    fn CVDisplayLinkSetOutputCallback(
        link: CVDisplayLinkRef,
        callback: CVOutputCallback,
        user_info: *mut c_void,
    ) -> i32;
    fn CVDisplayLinkStart(link: CVDisplayLinkRef) -> i32;
    fn CVDisplayLinkStop(link: CVDisplayLinkRef) -> i32;
}

/// Receives `coreVideoFrame:` on the main thread, and the run-loop modes it is
/// delivered in. Set once, before the link starts, and never released.
static TARGET: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());
static MODES: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());
/// Host time of the newest vsync (`f64` seconds as bits), and whether one is
/// queued.
static HOST_SECONDS: AtomicU64 = AtomicU64::new(0);
static QUEUED: AtomicBool = AtomicBool::new(false);

pub(super) struct Link(CVDisplayLinkRef);

impl Link {
    /// `target` must outlive the process and implement `coreVideoFrame:`.
    pub(super) fn new(target: &AnyObject) -> Option<Link> {
        let mut link: CVDisplayLinkRef = std::ptr::null_mut();
        unsafe {
            if CVDisplayLinkCreateWithActiveCGDisplays(&mut link) != 0 || link.is_null() {
                return None;
            }
            CVDisplayLinkSetOutputCallback(link, output, std::ptr::null_mut());
        }
        let modes: Retained<AnyObject> =
            unsafe { msg_send![objc2::class!(NSArray), arrayWithObject: NSDefaultRunLoopMode] };
        MODES.store(Retained::into_raw(modes), Ordering::Release);
        TARGET.store(
            target as *const AnyObject as *mut AnyObject,
            Ordering::Release,
        );
        Some(Link(link))
    }

    pub(super) fn set_display(&self, display: u32) {
        unsafe { CVDisplayLinkSetCurrentCGDisplay(self.0, display) };
    }

    /// Whether the link is now in the requested state; only a start can fail.
    pub(super) fn set_paused(&self, paused: bool) -> bool {
        unsafe {
            if paused {
                CVDisplayLinkStop(self.0);
                true
            } else {
                CVDisplayLinkStart(self.0) == 0
            }
        }
    }
}

thread_local! {
    /// The host time last handed to the main thread.
    static LAST_DELIVERED: Cell<f64> = const { Cell::new(f64::NEG_INFINITY) };
}

/// Called on the main thread for each forwarded vsync: its host time, in
/// seconds, or `None` when that vsync was already delivered. A vsync published
/// between clearing `QUEUED` and loading `HOST_SECONDS` queues a second
/// selector that would otherwise read the same time again.
pub(super) fn take_queued_host_seconds() -> Option<f64> {
    QUEUED.store(false, Ordering::Release);
    let seconds = f64::from_bits(HOST_SECONDS.load(Ordering::Acquire));
    LAST_DELIVERED.with(|last| {
        if seconds <= last.get() {
            return None;
        }
        last.set(seconds);
        Some(seconds)
    })
}

/// The latest vsync at or before `now`, on the grid of `output` (a later vsync)
/// spaced by `period`: the instant `CADisplayLink.timestamp` reports.
/// `inOutputTime` lies one or more periods ahead of `inNow`, and `inNow` itself
/// is when the callback thread woke, which jitters by a millisecond.
fn last_vsync(output: f64, now: f64, period: f64) -> f64 {
    if period <= 0.0 || output <= now {
        return output.min(now);
    }
    output - ((output - now) / period).ceil() * period
}

extern "C" fn output(
    _link: CVDisplayLinkRef,
    now: *const CVTimeStamp,
    output_time: *const CVTimeStamp,
    _flags_in: u64,
    _flags_out: *mut u64,
    _user_info: *mut c_void,
) -> i32 {
    let target = TARGET.load(Ordering::Acquire);
    let modes = MODES.load(Ordering::Acquire);
    if target.is_null() || modes.is_null() || now.is_null() || output_time.is_null() {
        return 0;
    }
    let (now, output) = unsafe { (&*now, &*output_time) };
    let period = if output.video_time_scale > 0 {
        output.video_refresh_period as f64 / output.video_time_scale as f64
    } else {
        0.0
    };
    let seconds = last_vsync(
        super::host_ticks_to_seconds(output.host_time),
        super::host_ticks_to_seconds(now.host_time),
        period,
    );
    HOST_SECONDS.store(seconds.to_bits(), Ordering::Release);
    if !QUEUED.swap(true, Ordering::AcqRel) {
        // This thread has no autorelease pool of its own.
        objc2::rc::autoreleasepool(|_| unsafe {
            let _: () = msg_send![
                target,
                performSelectorOnMainThread: objc2::sel!(coreVideoFrame:),
                withObject: std::ptr::null::<AnyObject>(),
                waitUntilDone: false,
                modes: modes
            ];
        });
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish(seconds: f64) {
        HOST_SECONDS.store(seconds.to_bits(), Ordering::Release);
        QUEUED.store(true, Ordering::Release);
    }

    #[test]
    fn the_timestamp_is_the_last_vsync_on_the_output_grid_not_in_the_future() {
        let period = 1.0 / 120.0;
        // Output one and two periods ahead of a `now` that woke 0.3 ms late.
        let vsync = 50.0;
        let now = vsync + 0.0003;
        for ahead in [1.0, 2.0] {
            let output = vsync + ahead * period;
            assert!((last_vsync(output, now, period) - vsync).abs() < 1e-9);
        }
        assert_eq!(last_vsync(vsync, vsync, period), vsync);
        assert_eq!(last_vsync(51.0, 50.5, 0.0), 50.5);
    }

    #[test]
    fn a_vsync_read_by_two_queued_selectors_is_delivered_once() {
        publish(100.0);
        assert_eq!(take_queued_host_seconds(), Some(100.0));
        assert_eq!(take_queued_host_seconds(), None);
        assert!(!QUEUED.load(Ordering::Acquire));

        publish(100.0 + 1.0 / 60.0);
        assert_eq!(take_queued_host_seconds(), Some(100.0 + 1.0 / 60.0));
    }
}
