//! CADisplayLink frame driver for iOS.
//!
//! Before this, `onFrame` callbacks were driven from the 8 ms `NSTimer` pump in
//! [`crate::app`] via `js_frame_pump_default()`, which timestamps with
//! `js_timer_now()`. That has two defects for anything frame-shaped:
//!
//! 1. **Not vsync-aligned.** An 8 ms repeating `NSTimer` free-runs against a
//!    8.333 ms (120 Hz) or 16.667 ms (60 Hz) display cadence and is subject to
//!    run-loop coalescing, so callbacks beat against the refresh rather than
//!    landing on it.
//! 2. **Millisecond-quantized.** `js_timer_now()` truncates to whole
//!    milliseconds, which is ~12% of a 120 Hz frame budget — enough to make a
//!    frame-time distribution meaningless.
//!
//! This module drives `js_frame_tick` from a real `CADisplayLink` instead, and
//! feeds [`perry_runtime::frame_metrics`] the vsync stream so frame-time
//! percentiles and dropped-frame counts can be measured on-device.
//!
//! # Run-loop mode
//!
//! The link is added in `NSRunLoopCommonModes`, not the default mode. During a
//! `UIScrollView` drag the run loop switches to `UITrackingRunLoopMode`; a link
//! registered only in the default mode goes silent for the whole gesture —
//! which is precisely the window a scrolling benchmark exists to measure.
//!
//! # Frame rate on ProMotion
//!
//! `CADisplayLink` defaults to an adaptive cadence on ProMotion displays, so a
//! mostly-static screen ticks well below 120 Hz. When metrics are enabled the
//! link requests the display's maximum rate so a benchmark measures the
//! framework rather than ProMotion's throttling.
//!
//! **On iPhone that request is capped at 60 Hz unless the app bundle sets
//! `CADisableMinimumFrameDurationOnPhone` to `YES` in its `Info.plist`.**
//! Perry's generated plist does not currently emit that key, so on-device
//! 120 Hz measurement needs it added first. iPad ProMotion is not subject to
//! that opt-in.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, AnyThread};
use objc2_foundation::{NSObject, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use std::cell::{Cell, RefCell};

use perry_runtime::frame::{js_frame_has_pending, js_frame_tick};
use perry_runtime::frame_metrics::{
    frame_metrics_enabled, js_frame_metrics_mark_discontinuity, js_frame_metrics_record,
};
use perry_runtime::timer::js_timer_now;

thread_local! {
    /// The installed link, retained so pause state can be updated from the
    /// timer pump. `None` until `install()` runs.
    static DISPLAY_LINK: RefCell<Option<Retained<CADisplayLink>>> = const { RefCell::new(None) };

    /// `(media_time_baseline_ms, runtime_clock_offset_ms)`, captured on the
    /// first tick. See `to_app_timeline_ms`.
    static TIME_BASELINE: Cell<Option<(f64, f64)>> = const { Cell::new(None) };

    /// Last pause state we applied, so `poll_pause_state` only touches the
    /// link (and only marks a discontinuity) on an actual transition.
    static PAUSED: Cell<bool> = const { Cell::new(false) };
}

/// Convert a `CADisplayLink` timestamp into the timeline `onFrame` documents.
///
/// `CADisplayLink.timestamp` is a `CFTimeInterval` on the `CACurrentMediaTime`
/// base — seconds since boot — while `onFrame`'s contract is "monotonic
/// milliseconds since app start". Handing the raw media time to JS would be a
/// silent contract break for any app doing `t - startTime`.
///
/// So the first tick pins the media clock to whatever `js_timer_now()` reads at
/// that moment, and every later tick is reported as an offset from that pin.
/// The result keeps the documented epoch while carrying the display link's
/// sub-microsecond resolution instead of `js_timer_now`'s whole milliseconds.
fn to_app_timeline_ms(media_time_ms: f64) -> f64 {
    let (baseline, offset) = TIME_BASELINE.with(|b| {
        if let Some(pair) = b.get() {
            pair
        } else {
            let pair = (media_time_ms, js_timer_now());
            b.set(Some(pair));
            pair
        }
    });
    (media_time_ms - baseline) + offset
}

pub struct PerryFrameLinkTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryFrameLinkTarget"]
    #[ivars = PerryFrameLinkTargetIvars]
    pub struct PerryFrameLinkTarget;

    impl PerryFrameLinkTarget {
        #[unsafe(method(frameTick:))]
        fn frame_tick(&self, sender: &AnyObject) {
            // The link fires straight into user JS via `js_frame_tick`, so it
            // needs the same panic guard the timer pump uses: a Rust panic
            // unwinding across the ObjC frame is undefined behaviour.
            crate::catch_callback_panic("frameTick", std::panic::AssertUnwindSafe(|| {
                let link: &CADisplayLink = unsafe { &*(sender as *const AnyObject as *const CADisplayLink) };

                let timestamp_ms = link.timestamp() * 1000.0;
                // `duration` is the display's nominal frame duration. It reads
                // 0 before the first tick completes; `js_frame_metrics_record`
                // keeps the last positive value, so an early 0 costs nothing.
                let expected_ms = link.duration() * 1000.0;

                let now_ms = to_app_timeline_ms(timestamp_ms);
                js_frame_metrics_record(now_ms, expected_ms);
                js_frame_tick(now_ms);
            }));
        }
    }
);

impl PerryFrameLinkTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryFrameLinkTargetIvars);
        unsafe { msg_send![super(this), init] }
    }
}

/// Whether the link should be ticking right now.
///
/// Metrics collection needs every vsync even with no `onFrame` subscribers —
/// a scrolling benchmark has zero frame callbacks and is exactly the workload
/// worth measuring — so enabled metrics keep the link awake on their own.
fn should_run() -> bool {
    js_frame_has_pending() != 0 || frame_metrics_enabled()
}

/// Install the display link on the main run loop. Call once, from app startup,
/// on the main thread.
pub fn install() {
    DISPLAY_LINK.with(|slot| {
        if slot.borrow().is_some() {
            return;
        }

        let target = PerryFrameLinkTarget::new();
        let link =
            unsafe { CADisplayLink::displayLinkWithTarget_selector(&target, sel!(frameTick:)) };

        // Common modes, so scroll tracking does not silence the link. See the
        // module docs.
        unsafe {
            link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
        }

        // Under measurement, ask for the display's maximum cadence rather than
        // ProMotion's adaptive default.
        if frame_metrics_enabled() {
            // Ask for the top of the ProMotion range and let the system clamp
            // to what the display can actually do — a 60 Hz device clamps this
            // to 60. Expressed as a range rather than read off `UIScreen`
            // because `UIScreen.mainScreen` is deprecated and the
            // instance-based replacement needs a view we do not have here.
            link.setPreferredFrameRateRange(CAFrameRateRange {
                minimum: 60.0,
                maximum: 120.0,
                preferred: 120.0,
            });
        }

        let paused = !should_run();
        link.setPaused(paused);
        PAUSED.with(|p| p.set(paused));

        // The target is referenced only weakly by the link, so it must outlive
        // this scope. The link runs for the process lifetime.
        std::mem::forget(target);
        *slot.borrow_mut() = Some(link);
    });
}

/// Reconcile the link's paused state with whether there is anything to do.
///
/// Driven from the timer pump rather than from the link itself: a paused link
/// gets no ticks, so it cannot observe a newly-registered `onFrame` callback
/// and wake itself up.
pub fn poll_pause_state() {
    let want_paused = !should_run();
    if PAUSED.with(|p| p.get()) == want_paused {
        return;
    }

    DISPLAY_LINK.with(|slot| {
        if let Some(link) = slot.borrow().as_ref() {
            link.setPaused(want_paused);
            PAUSED.with(|p| p.set(want_paused));
            // The idle gap across a pause is not a frame. Without this the
            // first tick after resuming reports the whole idle period as one
            // enormous interval and poisons the p99.
            js_frame_metrics_mark_discontinuity();
        }
    });
}
