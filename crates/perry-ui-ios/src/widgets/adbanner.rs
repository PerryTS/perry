//! `AdBanner` widget (#867) — iOS.
//!
//! Default build: a layout placeholder `UIView` sized to the requested
//! banner slot, so the banner reserves the right space in the layout
//! tree. Swapping in a live `GADBannerView` (set `adUnitID`, `adSize`,
//! `rootViewController`, then `loadRequest:`) is a feature-gated
//! follow-up that links the GoogleMobileAds framework via SwiftPM — see
//! `perry-ext-ads/src/ios_real.rs` for the same COMPILE-UNVERIFIED
//! caveat. `unitId` is accepted for API parity but unused by the
//! placeholder.
//!
//! NOTE: this crate cross-compiles only for the iOS target, which is not
//! present in host CI, so this file is compile-unverified here.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::UIView;

/// Extract a &str from a `*const StringHeader` pointer.
fn str_from_header(ptr: *const u8) -> &'static str {
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

/// Banner dimensions in points per size key (matches Google Mobile Ads'
/// standard `GADAdSize` constants).
fn banner_size(size_key: &str) -> (f64, f64) {
    match size_key {
        "large-banner" => (320.0, 100.0),
        "medium-rectangle" => (300.0, 250.0),
        "full-banner" => (468.0, 60.0),
        "leaderboard" => (728.0, 90.0),
        _ => (320.0, 50.0),
    }
}

/// Create the banner placeholder view sized per `size_ptr`.
pub fn create(unit_id_ptr: *const u8, size_ptr: *const u8) -> i64 {
    let _unit_id = str_from_header(unit_id_ptr);
    let size_key = str_from_header(size_ptr);
    let (w, h) = banner_size(size_key);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let view_cls = objc2::runtime::AnyClass::get(c"UIView").unwrap();
        let obj: *mut AnyObject = msg_send![view_cls, alloc];
        let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h));
        let obj: *mut AnyObject = msg_send![obj, initWithFrame: frame];
        // `initWithFrame:` already returns an owned (+1) object — take that
        // ownership with `from_raw` rather than `retain` (which would add a
        // second +1 and leak one reference per banner).
        let view: Retained<UIView> = Retained::from_raw(obj as *mut UIView).unwrap();
        super::register_widget(view)
    }
}
