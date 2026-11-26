//! `CVDisplayLink` frame source for macOS 13 and earlier, where AppKit has no
//! display-link factory. The link calls back on its own high-priority thread;
//! each vsync is forwarded to the main thread in the default run-loop mode, and
//! vsyncs that arrive while one is still queued are coalesced into it.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::NSDefaultRunLoopMode;
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

    pub(super) fn set_paused(&self, paused: bool) {
        unsafe {
            if paused {
                CVDisplayLinkStop(self.0);
            } else {
                CVDisplayLinkStart(self.0);
            }
        }
    }
}

/// Called on the main thread for each forwarded vsync: its host time, in
/// seconds.
pub(super) fn take_queued_host_seconds() -> f64 {
    QUEUED.store(false, Ordering::Release);
    f64::from_bits(HOST_SECONDS.load(Ordering::Acquire))
}

extern "C" fn output(
    _link: CVDisplayLinkRef,
    _now: *const CVTimeStamp,
    output_time: *const CVTimeStamp,
    _flags_in: u64,
    _flags_out: *mut u64,
    _user_info: *mut c_void,
) -> i32 {
    let target = TARGET.load(Ordering::Acquire);
    let modes = MODES.load(Ordering::Acquire);
    if target.is_null() || modes.is_null() || output_time.is_null() {
        return 0;
    }
    // `inOutputTime` is the next vsync, on the display's clock; one refresh
    // period earlier is the vsync that just happened, the instant
    // `CADisplayLink.timestamp` reports. `inNow` is when this thread woke,
    // which jitters by a millisecond.
    let output = unsafe { &*output_time };
    let period = if output.video_time_scale > 0 {
        output.video_refresh_period as f64 / output.video_time_scale as f64
    } else {
        0.0
    };
    let seconds = super::host_ticks_to_seconds(output.host_time) - period;
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
