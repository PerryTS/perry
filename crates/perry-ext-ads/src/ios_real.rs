//! Real iOS Google Mobile Ads bridge (#867) — **COMPILE-UNVERIFIED**.
//!
//! ⚠️  This module is gated behind `#[cfg(all(target_os = "ios",
//! feature = "google-ads"))]` and is **not built by host CI**. It makes
//! live objc2 calls into the GoogleMobileAds framework, which must be
//! supplied at link time via the SwiftPM `GoogleMobileAds` package /
//! `GoogleMobileAds.xcframework`. None of the code below has been
//! compiled — it cannot be, because:
//!   - the `aarch64-apple-ios` target + iOS SDK aren't present in the
//!     build environment that produced it, and
//!   - the GoogleMobileAds framework symbols aren't in the link graph
//!     until a consumer wires up the SwiftPM dependency.
//!
//! It is provided as a best-effort starting point for whoever finishes
//! the SwiftPM integration: the objc2 selector names, argument order,
//! and block signatures match the GoogleMobileAds Obj-C API as of SDK
//! v11, but MUST be validated against a real iOS build before shipping.
//! Treat every call site as needing review.
//!
//! Design notes / known gaps:
//!   - Uses dynamic dispatch (`class!` + `msg_send!`) rather than
//!     `extern_class!` bindings so the module doesn't need a generated
//!     GAD type surface.
//!   - Each completion block resolves the promise exactly once. The
//!     promise is consumed on resolve, so it's captured in a
//!     `RefCell<Option<JsPromise>>` and `take()`n on first fire (these
//!     GAD/ATT completion handlers fire once, on the main thread).
//!   - Loaded ads are cached in thread-locals (single slot each) —
//!     matches the `load once / show once` FFI shape.
//!   - `*_show` presents from the key window's root view controller and
//!     resolves on a successful present. Accurate `dismissed` reporting
//!     needs a `GADFullScreenContentDelegate` (an objc2 `define_class!`
//!     delegate) — that's a follow-up; for now `dismissed` is reported
//!     `false` with no error on a successful present.

use std::cell::RefCell;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

use crate::{
    resolve_interstitial_show_failure, resolve_load_failure, resolve_rewarded_show_failure,
    JsPromise,
};

thread_local! {
    /// The most-recently-loaded interstitial, retained until shown.
    static INTERSTITIAL: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    /// The most-recently-loaded rewarded ad, retained until shown.
    static REWARDED: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// Resolve a `JsPromise` captured in a once-firing completion block.
/// No-op if the block somehow fires twice (the promise is already gone).
fn settle(slot: &RefCell<Option<JsPromise>>, json: &str) {
    if let Some(p) = slot.borrow_mut().take() {
        p.resolve_string(json);
    }
}

/// Build a fresh `GADRequest`.
unsafe fn new_request() -> Retained<AnyObject> {
    msg_send![class!(GADRequest), request]
}

/// Resolve the key window's `rootViewController` for ad presentation.
/// Returns null if no window is active (the present then no-ops).
unsafe fn root_view_controller() -> *mut AnyObject {
    let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
    if app.is_null() {
        return std::ptr::null_mut();
    }
    // `keyWindow` is deprecated but still the simplest single-scene
    // lookup; a scene-aware lookup is a refinement.
    let window: *mut AnyObject = msg_send![app, keyWindow];
    if window.is_null() {
        return std::ptr::null_mut();
    }
    msg_send![window, rootViewController]
}

// ---------------------------------------------------------------------
// Interstitial
// ---------------------------------------------------------------------

pub fn interstitial_load(promise: JsPromise, unit_id: String) {
    unsafe {
        let ns_unit = NSString::from_str(&unit_id);
        let request = new_request();
        let slot = RefCell::new(Some(promise));

        // completionHandler: void (^)(GADInterstitialAd *, NSError *)
        let handler = RcBlock::new(move |ad: *mut AnyObject, err: *mut AnyObject| {
            if !err.is_null() || ad.is_null() {
                if let Some(p) = slot.borrow_mut().take() {
                    resolve_load_failure(p, "no-fill");
                }
                return;
            }
            INTERSTITIAL.with(|s| *s.borrow_mut() = Retained::retain(ad));
            settle(&slot, r#"{"success":true}"#);
        });

        let _: () = msg_send![
            class!(GADInterstitialAd),
            loadWithAdUnitID: &*ns_unit,
            request: &*request,
            completionHandler: &*handler,
        ];
    }
}

pub fn interstitial_show(promise: JsPromise) {
    unsafe {
        let ad = INTERSTITIAL.with(|slot| slot.borrow_mut().take());
        let Some(ad) = ad else {
            resolve_interstitial_show_failure(promise, "not-loaded");
            return;
        };
        let rvc = root_view_controller();
        if rvc.is_null() {
            resolve_interstitial_show_failure(promise, "no-root-view-controller");
            return;
        }
        let _: () = msg_send![&*ad, presentFromRootViewController: rvc];
        // NOTE: resolves on present, not on dismissal — see module header.
        promise.resolve_string(r#"{"shown":true,"dismissed":false}"#);
    }
}

// ---------------------------------------------------------------------
// Rewarded
// ---------------------------------------------------------------------

pub fn rewarded_load(promise: JsPromise, unit_id: String) {
    unsafe {
        let ns_unit = NSString::from_str(&unit_id);
        let request = new_request();
        let slot = RefCell::new(Some(promise));

        let handler = RcBlock::new(move |ad: *mut AnyObject, err: *mut AnyObject| {
            if !err.is_null() || ad.is_null() {
                if let Some(p) = slot.borrow_mut().take() {
                    resolve_load_failure(p, "no-fill");
                }
                return;
            }
            REWARDED.with(|s| *s.borrow_mut() = Retained::retain(ad));
            settle(&slot, r#"{"success":true}"#);
        });

        let _: () = msg_send![
            class!(GADRewardedAd),
            loadWithAdUnitID: &*ns_unit,
            request: &*request,
            completionHandler: &*handler,
        ];
    }
}

pub fn rewarded_show(promise: JsPromise) {
    unsafe {
        let ad = REWARDED.with(|slot| slot.borrow_mut().take());
        let Some(ad) = ad else {
            resolve_rewarded_show_failure(promise, "not-loaded");
            return;
        };
        let rvc = root_view_controller();
        if rvc.is_null() {
            resolve_rewarded_show_failure(promise, "no-root-view-controller");
            return;
        }
        // userDidEarnRewardHandler: void (^)(void) — the reward is read
        // from `ad.adReward` inside the handler.
        let ad_for_handler = Retained::clone(&ad);
        let slot = RefCell::new(Some(promise));
        let reward_handler = RcBlock::new(move || {
            let reward: *mut AnyObject = msg_send![&*ad_for_handler, adReward];
            let amount: f64 = if reward.is_null() {
                0.0
            } else {
                // `amount` is an NSDecimalNumber; `doubleValue` is good
                // enough for a reward count.
                msg_send![reward, doubleValue]
            };
            settle(
                &slot,
                &format!(r#"{{"earned":true,"dismissed":true,"amount":{}}}"#, amount),
            );
        });
        let _: () = msg_send![
            &*ad,
            presentFromRootViewController: rvc,
            userDidEarnRewardHandler: &*reward_handler,
        ];
    }
}

// ---------------------------------------------------------------------
// Consent (ATT)
// ---------------------------------------------------------------------

pub fn consent_request(promise: JsPromise) {
    unsafe {
        // ATTrackingManager.requestTrackingAuthorization(completionHandler:)
        // completion receives ATTrackingManagerAuthorizationStatus (NSUInteger):
        //   0 notDetermined, 1 restricted, 2 denied, 3 authorized.
        //
        // ATTrackingManager only exists on iOS 14+. On older OSes the class
        // is absent and the completion block would never fire, hanging the
        // promise forever — so look the class up dynamically and resolve
        // immediately with `not-determined` when it's missing.
        let Some(cls) = objc2::runtime::AnyClass::get(c"ATTrackingManager") else {
            crate::resolve_consent_failure(promise, "att-unavailable");
            return;
        };
        let slot = RefCell::new(Some(promise));
        let handler = RcBlock::new(move |status: usize| {
            let slug = match status {
                3 => "authorized",
                2 => "denied",
                1 => "restricted",
                _ => "not-determined",
            };
            settle(&slot, &format!(r#"{{"status":"{}"}}"#, slug));
        });
        let _: () = msg_send![
            cls,
            requestTrackingAuthorizationWithCompletionHandler: &*handler,
        ];
    }
}
