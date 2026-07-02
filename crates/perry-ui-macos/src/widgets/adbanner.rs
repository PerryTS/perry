//! `AdBanner` widget (#867).
//!
//! Google Mobile Ads ships no first-party macOS SDK, so on the Mac the
//! banner is a **layout placeholder**: an empty `NSView` sized to the
//! requested banner slot. A `perry/ui` layout developed or previewed on
//! macOS therefore reserves exactly the space the real ad occupies on
//! iOS/Android (where the banner widget renders a live `GADBannerView` /
//! `AdView`). The `unitId` is accepted for cross-platform API parity but
//! unused here since nothing loads.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::MainThreadMarker;

use super::image::str_from_header;

/// Banner dimensions in points for each size key the d.ts exposes.
/// Values match Google Mobile Ads' standard `AdSize` constants so the
/// reserved macOS slot lines up with the real iOS/Android banner.
fn banner_size(size_key: &str) -> (f64, f64) {
    match size_key {
        "large-banner" => (320.0, 100.0),
        "medium-rectangle" => (300.0, 250.0),
        "full-banner" => (468.0, 60.0),
        "leaderboard" => (728.0, 90.0),
        // "banner" / "adaptive" / empty / unknown → standard 320×50.
        _ => (320.0, 50.0),
    }
}

/// Create the banner placeholder view sized per `size_ptr`. Returns the
/// widget handle.
pub fn create(unit_id_ptr: *const u8, size_ptr: *const u8) -> i64 {
    let _unit_id = str_from_header(unit_id_ptr);
    let size_key = str_from_header(size_ptr);
    let (w, h) = banner_size(size_key);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h));
        let view: Retained<NSView> = msg_send![NSView::alloc(mtm), initWithFrame: frame];
        super::register_widget(view)
    }
}
