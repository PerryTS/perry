//! `perry/ads` native bindings (issue #867).
//!
//! TypeScript surface:
//!
//! ```ignore
//! // Interstitial — load once, show on demand.
//! export declare function js_ads_interstitial_load(unitId: string): Promise<string>;
//! export declare function js_ads_interstitial_show(): Promise<string>;
//!
//! // Rewarded — load once, show on demand, JSON result includes
//! // reward amount.
//! export declare function js_ads_rewarded_load(unitId: string): Promise<string>;
//! export declare function js_ads_rewarded_show(): Promise<string>;
//!
//! // Banner — handle-based widget. The primary banner surface is the
//! // `perry/ui` `<AdBanner>` widget (it sits in the declarative layout
//! // tree); this raw FFI is the secondary, handle-based path.
//! export declare function js_ads_banner_create(unitId: string, sizeKey: string): number;
//! export declare function js_ads_banner_destroy(handle: number): void;
//!
//! // Consent — ATT (iOS) / UMP (Android). Call before loading ads.
//! export declare function js_ads_request_consent(): Promise<string>;
//! ```
//!
//! The promise-returning entry points each resolve to a JSON
//! string rather than a structured object so the schema can grow
//! (new fields, new error slugs, real reward metadata) without
//! breaking the native ABI. The shape mirrors what real SDK
//! integration will emit, just with the values pinned to the
//! "ad not shown / not earned" placeholder:
//!
//! ```json
//! // *_load resolves:
//! {"success": false, "error": "no-sdk-linked"}
//!
//! // interstitial *_show resolves:
//! {"shown": false, "dismissed": false, "error": "no-sdk-linked"}
//!
//! // rewarded *_show resolves:
//! {"earned": false, "dismissed": false, "error": "no-sdk-linked"}
//! ```
//!
//! # Platform coverage
//!
//! - **iOS / Mac Catalyst** (`target_os = "ios"`): a *supported*
//!   platform. Default builds resolve `"no-sdk-linked"`; building with
//!   `--features google-ads` delegates to `ios_real`, which makes live
//!   objc2 calls into the Google Mobile Ads SDK (`GADInterstitialAd`,
//!   `GADRewardedAd`) and `ATTrackingManager` for consent. That module
//!   is COMPILE-UNVERIFIED (needs the SwiftPM xcframework + iOS target)
//!   — see `ios_real.rs`. The ATT prompt needs the per-app
//!   `NSUserTrackingUsageDescription` Info.plist key, which the compiler
//!   emits from `perry.toml [ads].att_usage_description` (#867).
//! - **Android**: a *supported* platform; resolves `"no-sdk-linked"`
//!   until the JNI bridge into `com.google.android.gms:play-services-ads`
//!   (+ UMP consent) lands. The compiler emits the `APPLICATION_ID`
//!   meta-data from `perry.toml [ads].android_app_id`.
//! - **macOS (non-Catalyst) / Linux / Windows / tvOS / watchOS /
//!   visionOS / gtk4**: no first-party Google Mobile Ads SDK exists, so
//!   every call resolves `{ error: "unsupported-platform" }`. (A
//!   `perry/ui` app developed/previewed on macOS native lands here.)
//!
//! # Configuration
//!
//! `perry.toml` (follow-up — not consumed by this crate yet):
//!
//! ```toml
//! [ads]
//! ios_app_id = "ca-app-pub-XXXX~YYYY"
//! android_app_id = "ca-app-pub-XXXX~ZZZZ"
//! test_device_ids = ["DEVICE_HASH_HERE"]
//! request_non_personalized_ads_only = false
//! ```
//!
//! The block is parsed in `crates/perry/src/commands/compile.rs`
//! (follow-up) and surfaced to the native SDK via Info.plist
//! `GADApplicationIdentifier` on Apple targets and
//! `AndroidManifest.xml` `<meta-data android:name="com.google.android.gms.ads.APPLICATION_ID">`
//! on Android. The MVP entry points here don't yet consume the
//! config — they only need to compile + link.
//!
//! # Why structured failures rather than a Rust `Result`?
//!
//! The JS surface is `Promise<string>`. The promise never
//! rejects — every outcome (load failed, no fill, user dismissed
//! without watching, ATT denied, GDPR consent withheld) flows
//! through the same JSON shape so callers handle every case at
//! one `await` site without juggling `try`/`catch` and
//! `.then(onFulfilled, onRejected)` branches. The bcrypt / argon2
//! / google-auth wrappers follow the same convention.

use perry_ffi::{read_string, spawn_blocking, JsPromise, JsString, StringHeader};

// =====================================================================
// Shared helpers — each platform path funnels through these so the
// JSON shapes stay consistent.
// =====================================================================

/// Resolve `promise` with a `*_load` failure result. The JSON
/// shape is `{"success": false, "error": "<slug>"}`.
///
/// Resolution runs on the FFI runtime's blocking executor so
/// the caller's `.then` / `await` has time to register before
/// the value lands. Resolving synchronously inside the FFI
/// function would deliver the value before the JS side hooks the
/// promise up, and the microtask never fires — same trap as
/// bcrypt / argon2 / fetch.
fn resolve_load_failure(promise: JsPromise, error: &'static str) {
    spawn_blocking(move || {
        // Static slug → no escaping needed. If a future slug
        // contains `"` or `\`, switch to `serde_json::to_string`.
        let json = format!(r#"{{"success":false,"error":"{}"}}"#, error);
        promise.resolve_string(&json);
    });
}

/// Resolve `promise` with an interstitial `*_show` failure
/// result. JSON shape: `{"shown": false, "dismissed": false, "error": "<slug>"}`.
fn resolve_interstitial_show_failure(promise: JsPromise, error: &'static str) {
    spawn_blocking(move || {
        let json = format!(r#"{{"shown":false,"dismissed":false,"error":"{}"}}"#, error);
        promise.resolve_string(&json);
    });
}

/// Resolve `promise` with a rewarded `*_show` failure result.
/// JSON shape: `{"earned": false, "dismissed": false, "error": "<slug>"}`.
fn resolve_rewarded_show_failure(promise: JsPromise, error: &'static str) {
    spawn_blocking(move || {
        let json = format!(
            r#"{{"earned":false,"dismissed":false,"error":"{}"}}"#,
            error
        );
        promise.resolve_string(&json);
    });
}

/// Resolve `promise` with a consent-request result. JSON shape:
/// `{"status": "not-determined", "error": "<slug>"}`.
///
/// `status` mirrors what the real ATT (App Tracking Transparency, iOS) /
/// UMP (User Messaging Platform, Android) flow reports — `"authorized"`,
/// `"denied"`, `"not-determined"`, `"restricted"`. Until the SDK is
/// linked it's pinned to `"not-determined"` with an `error` slug so
/// callers can branch on either field.
fn resolve_consent_failure(promise: JsPromise, error: &'static str) {
    spawn_blocking(move || {
        let json = format!(r#"{{"status":"not-determined","error":"{}"}}"#, error);
        promise.resolve_string(&json);
    });
}

// =====================================================================
// Apple (iOS + Mac Catalyst — `target_os = "ios"` covers device,
// simulator, and Mac Catalyst)
// =====================================================================
//
// Real SDK integration goes through the Google Mobile Ads SwiftPM
// package — `GADMobileAds.sharedInstance().start(completionHandler:)`
// once at app launch, then
// `GADInterstitialAd.load(withAdUnitID:request:completionHandler:)`
// for the interstitial path,
// `GADRewardedAd.load(withAdUnitID:request:completionHandler:)`
// for rewarded, and `GADBannerView` (with `adSize` /
// `rootViewController`) for the banner widget. ATT consent goes
// through `ATTrackingManager.requestTrackingAuthorization`.
//
// Two build modes:
//   - default: route every call to the structured failure helper
//     (`no-sdk-linked`) so the crate compiles on `cargo build`
//     without dragging the SwiftPM SDK into the link graph. iOS is a
//     *supported* platform, so the slug is `no-sdk-linked`, not
//     `unsupported-platform`.
//   - `--features google-ads`: delegate to `ios_real`, which makes
//     real objc2 calls into the GoogleMobileAds framework. That module
//     is COMPILE-UNVERIFIED here (it needs the iOS target + the
//     xcframework, neither present in host CI) — see its header.

#[cfg(all(target_os = "ios", feature = "google-ads"))]
mod ios_real;

#[cfg(target_os = "ios")]
mod platform {
    use super::*;

    pub fn interstitial_load(promise: JsPromise, unit_id: String) {
        #[cfg(feature = "google-ads")]
        {
            super::ios_real::interstitial_load(promise, unit_id);
        }
        #[cfg(not(feature = "google-ads"))]
        {
            let _ = unit_id;
            resolve_load_failure(promise, "no-sdk-linked");
        }
    }

    pub fn interstitial_show(promise: JsPromise) {
        #[cfg(feature = "google-ads")]
        {
            super::ios_real::interstitial_show(promise);
        }
        #[cfg(not(feature = "google-ads"))]
        {
            resolve_interstitial_show_failure(promise, "no-sdk-linked");
        }
    }

    pub fn rewarded_load(promise: JsPromise, unit_id: String) {
        #[cfg(feature = "google-ads")]
        {
            super::ios_real::rewarded_load(promise, unit_id);
        }
        #[cfg(not(feature = "google-ads"))]
        {
            let _ = unit_id;
            resolve_load_failure(promise, "no-sdk-linked");
        }
    }

    pub fn rewarded_show(promise: JsPromise) {
        #[cfg(feature = "google-ads")]
        {
            super::ios_real::rewarded_show(promise);
        }
        #[cfg(not(feature = "google-ads"))]
        {
            resolve_rewarded_show_failure(promise, "no-sdk-linked");
        }
    }

    /// TODO(#867): create a `GADBannerView`, attach it to the root
    /// view controller, register it in the perry-ffi handle table,
    /// and return the handle. (The `perry/ui` `<AdBanner>` widget is
    /// the primary banner surface; this raw-FFI path is secondary.)
    pub fn banner_create(_unit_id: String, _size_key: String) -> i64 {
        // Placeholder handle. Real impl will call
        // `perry_ffi::register_handle(banner_view)`.
        0
    }

    pub fn banner_destroy(_handle: i64) {
        // No-op until real SDK integration lands.
    }

    /// ATT consent — `ATTrackingManager.requestTrackingAuthorization`.
    pub fn consent_request(promise: JsPromise) {
        #[cfg(feature = "google-ads")]
        {
            super::ios_real::consent_request(promise);
        }
        #[cfg(not(feature = "google-ads"))]
        {
            resolve_consent_failure(promise, "no-sdk-linked");
        }
    }
}

// =====================================================================
// Android
// =====================================================================
//
// Real impl bridges to `com.google.android.gms.ads.*` via JNI into the
// Kotlin helpers in
// `crates/perry-ui-android/template/.../PerryBridge.kt`
// (`adsInterstitialLoad` / `adsInterstitialShow` / `adsRewardedLoad` /
// `adsRewardedShow` / `adsRequestConsent`). The completion side
// resolves the promise once the Kotlin callback fires. The JNI wiring
// is a follow-up; for now Android is a *supported* platform whose calls
// resolve `no-sdk-linked`.

#[cfg(target_os = "android")]
mod platform {
    use super::*;

    pub fn interstitial_load(promise: JsPromise, _unit_id: String) {
        resolve_load_failure(promise, "no-sdk-linked");
    }

    pub fn interstitial_show(promise: JsPromise) {
        resolve_interstitial_show_failure(promise, "no-sdk-linked");
    }

    pub fn rewarded_load(promise: JsPromise, _unit_id: String) {
        resolve_load_failure(promise, "no-sdk-linked");
    }

    pub fn rewarded_show(promise: JsPromise) {
        resolve_rewarded_show_failure(promise, "no-sdk-linked");
    }

    pub fn banner_create(_unit_id: String, _size_key: String) -> i64 {
        0
    }

    pub fn banner_destroy(_handle: i64) {
        // No-op until real SDK integration lands.
    }

    /// UMP consent — `ConsentInformation.requestConsentInfoUpdate`.
    pub fn consent_request(promise: JsPromise) {
        resolve_consent_failure(promise, "no-sdk-linked");
    }
}

// =====================================================================
// Other targets — macOS (non-Catalyst) / Linux / Windows / tvOS /
// watchOS / visionOS / gtk4
// =====================================================================
//
// Google Mobile Ads ships no first-party SDK on any of these, so every
// call resolves `unsupported-platform`. This is the honest distinction
// the MVP deferred: iOS/Android say `no-sdk-linked` (a supported
// platform whose SDK isn't wired up yet); everything else — including
// macOS native, where a `perry/ui` app may be developed/previewed —
// says `unsupported-platform`.

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod platform {
    use super::*;

    pub fn interstitial_load(promise: JsPromise, _unit_id: String) {
        resolve_load_failure(promise, "unsupported-platform");
    }

    pub fn interstitial_show(promise: JsPromise) {
        resolve_interstitial_show_failure(promise, "unsupported-platform");
    }

    pub fn rewarded_load(promise: JsPromise, _unit_id: String) {
        resolve_load_failure(promise, "unsupported-platform");
    }

    pub fn rewarded_show(promise: JsPromise) {
        resolve_rewarded_show_failure(promise, "unsupported-platform");
    }

    pub fn banner_create(_unit_id: String, _size_key: String) -> i64 {
        0
    }

    pub fn banner_destroy(_handle: i64) {
        // No-op — no SDK on this platform.
    }

    pub fn consent_request(promise: JsPromise) {
        resolve_consent_failure(promise, "unsupported-platform");
    }
}

// =====================================================================
// FFI surface — names match the d.ts (`types/perry/ads/index.d.ts`).
// =====================================================================
//
// Promise-returning functions allocate a `JsPromise` synchronously,
// snapshot its raw pointer, dispatch the resolution work onto the
// FFI blocking executor, and return the raw pointer immediately —
// same shape every other promise-based `perry-ext-*` crate uses.
//
// String args arrive as `*const StringHeader` — the codegen
// `NA_STR` lowering calls `js_get_string_pointer_unified` on the
// NaN-boxed input and forwards the i64 pointer to the FFI. We
// read it through `perry_ffi::read_string` (handles null + UTF-8
// validation), then own a copy before passing it across the
// `spawn_blocking` boundary.

/// Read a `*const StringHeader` arg as an owned `String`.
///
/// Returns `String::new()` on null or invalid-UTF-8 input so the
/// FFI is total — every caller is happy with an empty unit ID
/// flowing through to the structured-failure path rather than a
/// hard error mid-link.
///
/// # Safety
///
/// `ptr` must either be null or point to a valid runtime-allocated
/// `StringHeader`. The runtime guarantees this when it dispatches
/// `NA_STR` args.
unsafe fn str_arg_to_owned(ptr: *const StringHeader) -> String {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(str::to_owned).unwrap_or_default()
}

/// `js_ads_interstitial_load(unitId)` — preload an interstitial
/// against `unitId`. Resolves to a JSON-stringified
/// `AdLoadResult`.
#[no_mangle]
pub unsafe extern "C" fn js_ads_interstitial_load(
    unit_id: *const StringHeader,
) -> *mut perry_ffi::Promise {
    let unit_id = str_arg_to_owned(unit_id);
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    platform::interstitial_load(promise, unit_id);
    raw
}

/// `js_ads_interstitial_show()` — present the previously-loaded
/// interstitial. Resolves to a JSON-stringified
/// `AdShowResult` (interstitial variant).
#[no_mangle]
pub extern "C" fn js_ads_interstitial_show() -> *mut perry_ffi::Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    platform::interstitial_show(promise);
    raw
}

/// `js_ads_rewarded_load(unitId)` — preload a rewarded ad.
#[no_mangle]
pub unsafe extern "C" fn js_ads_rewarded_load(
    unit_id: *const StringHeader,
) -> *mut perry_ffi::Promise {
    let unit_id = str_arg_to_owned(unit_id);
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    platform::rewarded_load(promise, unit_id);
    raw
}

/// `js_ads_rewarded_show()` — present the previously-loaded
/// rewarded ad. Resolves to a JSON-stringified `AdShowResult`
/// (rewarded variant — includes optional `amount` / `type` fields
/// when the user earned the reward).
#[no_mangle]
pub extern "C" fn js_ads_rewarded_show() -> *mut perry_ffi::Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    platform::rewarded_show(promise);
    raw
}

/// `js_ads_banner_create(unitId, sizeKey)` — allocate a banner
/// widget against the SDK and return a perry-ffi handle. v1
/// exposes the handle directly; perry/ui `<AdBanner>` integration
/// is a follow-up so the widget can sit inside the declarative
/// layout tree.
///
/// `sizeKey` is one of `"banner"` / `"large-banner"` /
/// `"medium-rectangle"` / `"adaptive"` (matches Google Mobile
/// Ads' `GADAdSizeBanner` / `GADAdSizeLargeBanner` / etc.). The
/// MVP doesn't parse it — that's part of the real-SDK follow-up.
///
/// The return type is `f64` (Perry's `number`) rather than `i64`
/// so the result NaN-boxes through `NR_F64` codegen without a
/// pointer-tag detour. Small integer handle IDs round-trip
/// losslessly through f64 mantissa.
#[no_mangle]
pub unsafe extern "C" fn js_ads_banner_create(
    unit_id: *const StringHeader,
    size_key: *const StringHeader,
) -> f64 {
    let unit_id = str_arg_to_owned(unit_id);
    let size_key = str_arg_to_owned(size_key);
    platform::banner_create(unit_id, size_key) as f64
}

/// `js_ads_banner_destroy(handle)` — release the banner.
///
/// Arg type is `f64` (matches the codegen `NA_F64` calling
/// convention for `number` parameters). We narrow to `i64` for
/// the handle-table lookup.
#[no_mangle]
pub extern "C" fn js_ads_banner_destroy(handle: f64) {
    platform::banner_destroy(handle as i64);
}

/// `js_ads_request_consent()` — request user consent for personalised
/// ads / tracking. Resolves to a JSON-stringified `AdConsentResult`.
///
/// On iOS this drives App Tracking Transparency
/// (`ATTrackingManager.requestTrackingAuthorization`); on Android the
/// User Messaging Platform consent form
/// (`ConsentInformation`/`ConsentForm`). The result `status` is one of
/// `"authorized"` / `"denied"` / `"not-determined"` / `"restricted"`.
/// Call this before loading ads so requests honour GDPR / ATT.
#[no_mangle]
pub extern "C" fn js_ads_request_consent() -> *mut perry_ffi::Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    platform::consent_request(promise);
    raw
}

// No `#[cfg(test)] mod tests` block here: the FFI entry points
// allocate through `perry_ffi_promise_new`, a symbol provided by
// perry-stdlib at final-binary link time. A standalone
// `cargo test -p perry-ext-ads` (no stdlib in the link graph)
// would fail to link, matching every other promise-based
// `perry-ext-*` wrapper (bcrypt, argon2, fetch, google-auth, …).
// The smoke test under `test-files/test_ads_compile_smoke.ts`
// exercises the surface end-to-end against the linked binary.
