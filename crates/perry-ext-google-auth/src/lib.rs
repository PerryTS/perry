//! `@perryts/google-auth` native bindings (issue #674).
//!
//! TypeScript surface:
//!
//! ```ignore
//! export declare function js_google_auth_sign_in(): Promise<string>;
//! export declare function js_google_auth_silent_sign_in(): Promise<string>;
//! export declare function js_google_auth_sign_out(): Promise<string>;
//! ```
//!
//! Each function resolves to a JSON-stringified
//! [`GoogleSignInResult`]. The runtime returns a `Promise<string>`
//! rather than a structured object so the JSON shape can grow
//! (additional fields, new error codes) without forcing every
//! caller through a runtime-versioned native struct contract.
//!
//! # Platform coverage (MVP)
//!
//! - **iOS / Mac Catalyst / macOS 13+**: real impl bridges to the
//!   GoogleSignIn SDK via SwiftPM + objc2 lives in
//!   [`mod platform`] under `cfg(any(target_os = "ios", target_os = "macos"))`.
//!   For the MVP it resolves a structured
//!   `{ success: false, error: "not-yet-implemented" }` so the
//!   crate compiles + links without dragging SwiftPM into
//!   `cargo build` and so downstream gating (perry-api-manifest,
//!   `@perryts/google-auth` d.ts surface, smoke tests) can land
//!   independently. The SDK calls themselves are a follow-up.
//! - **Android**: real impl bridges to
//!   `androidx.credentials.CredentialManager + GetGoogleIdOption`
//!   via JNI; same MVP placeholder pattern as iOS.
//! - **Linux / Windows**: stub for v1 — resolves
//!   `{ success: false, error: "unsupported-platform" }`. The
//!   desktop story is a system-browser + loopback OAuth flow
//!   tracked as a follow-up.
//! - **tvOS / watchOS / visionOS / gtk4**: no-op stub, same shape
//!   as Linux/Windows. Google Sign In has no first-party SDK on
//!   these targets and the issue explicitly defers them.
//!
//! # Configuration
//!
//! `perry.toml`:
//!
//! ```toml
//! [google_auth]
//! ios_client_id = "..."
//! android_client_id = "..."
//! server_client_id = "..."         # for backend ID-token verification
//! default_scopes = ["openid", "email", "profile"]
//! ```
//!
//! The TOML block is parsed in
//! `crates/perry/src/commands/compile.rs` and surfaced to the
//! native impl via platform-specific paths (Info.plist on Apple,
//! AndroidManifest meta-data on Android). MVP: the FFI functions
//! here do not yet consume the config — they only need to compile
//! + link. See #674 follow-up tasks.

use perry_ffi::{spawn_blocking, JsPromise};

/// Resolve the given promise with a JSON-stringified failure result.
/// Used by every platform path in the MVP.
///
/// The resolution is dispatched through `spawn_blocking` — same
/// pattern bcrypt / argon2 / fetch / every other promise-returning
/// ext crate uses — so the resolution runs after the caller has
/// registered any `.then` / `await`. Resolving synchronously
/// before the FFI function even returns would land the resolved
/// value before the JS side hooks the promise up, and the
/// microtask never fires.
///
/// The `error` slug is the value of the `"error"` field in the
/// returned `GoogleSignInResult`.
fn resolve_failure(promise: JsPromise, error: &'static str) {
    spawn_blocking(move || {
        // The JSON encoder here is intentionally trivial — the only
        // input is a static slug. We escape nothing because none of
        // the slugs below contain `"` or `\`. If you add a slug
        // that does, switch this to a real `serde_json::to_string`
        // call.
        let json = format!(r#"{{"success":false,"error":"{}"}}"#, error);
        promise.resolve_string(&json);
    });
}

// =====================================================================
// Apple (iOS + Mac Catalyst + macOS 13+)
// =====================================================================
//
// Real SDK integration goes through the GoogleSignIn SwiftPM
// package and uses `GIDSignIn.sharedInstance.signIn` /
// `restorePreviousSignIn` / `signOut`. The objc2 bridge will live
// in this module. For the MVP we land the module boundary but
// route every call to `resolve_failure` so the crate compiles
// on `cargo build` without the SDK on PATH.

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod platform {
    use super::*;

    /// Issue #674 follow-up — wire to `GIDSignIn.sharedInstance.signIn`.
    /// The objc2 call shape (per CLAUDE.md objc2 v0.6 conventions):
    ///
    /// ```ignore
    /// let cls = objc2::runtime::AnyClass::get(c"GIDSignIn").unwrap();
    /// let shared: *mut AnyObject = msg_send![cls, sharedInstance];
    /// msg_send![shared, signInWithPresentingViewController: vc,
    ///                   completion: completion_block];
    /// ```
    ///
    /// The presenting view controller comes from
    /// `perry_ui_*_root_view_controller()` (see perry-ui-ios /
    /// perry-ui-macos). The completion block resolves the promise
    /// from the main thread.
    pub fn sign_in(promise: JsPromise) {
        resolve_failure(promise, "not-yet-implemented");
    }

    /// `restorePreviousSignIn` — the silent / refresh-token flow.
    pub fn silent_sign_in(promise: JsPromise) {
        resolve_failure(promise, "not-yet-implemented");
    }

    /// `GIDSignIn.sharedInstance.signOut()` — synchronous on the
    /// SDK side, but we still resolve through a promise so the
    /// JS surface stays uniform across platforms.
    pub fn sign_out(promise: JsPromise) {
        resolve_failure(promise, "not-yet-implemented");
    }
}

// =====================================================================
// Android
// =====================================================================
//
// Real impl bridges to `androidx.credentials.CredentialManager` —
// specifically `getCredential(GetCredentialRequest(GetGoogleIdOption))`.
// The Kotlin side lives in
// `crates/perry-ui-android/template/.../PerryBridge.kt` (see the
// `googleAuthSignIn` / `googleAuthSilentSignIn` / `googleAuthSignOut`
// JNI entry points). The JNI plumbing here resolves the promise
// once the Kotlin completion fires.

#[cfg(target_os = "android")]
mod platform {
    use super::*;

    pub fn sign_in(promise: JsPromise) {
        resolve_failure(promise, "not-yet-implemented");
    }

    pub fn silent_sign_in(promise: JsPromise) {
        resolve_failure(promise, "not-yet-implemented");
    }

    pub fn sign_out(promise: JsPromise) {
        resolve_failure(promise, "not-yet-implemented");
    }
}

// =====================================================================
// Other targets (linux/gtk4, windows, tvos/watchos/visionos)
// =====================================================================
//
// `cfg(not(any(...)))` is brittle as the target list grows — we
// instead negate the platforms that have a `platform` module
// above. Any target that doesn't match the two cfgs above gets
// the `unsupported-platform` stub.

#[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
mod platform {
    use super::*;

    pub fn sign_in(promise: JsPromise) {
        resolve_failure(promise, "unsupported-platform");
    }

    pub fn silent_sign_in(promise: JsPromise) {
        resolve_failure(promise, "unsupported-platform");
    }

    pub fn sign_out(promise: JsPromise) {
        resolve_failure(promise, "unsupported-platform");
    }
}

// =====================================================================
// FFI surface — names match the d.ts (`types/perry/google-auth/index.d.ts`).
// =====================================================================

/// `js_google_auth_sign_in()` — interactive Google Sign In.
///
/// Returns a `Promise<string>` that resolves to a JSON-stringified
/// `GoogleSignInResult`. The promise never rejects: cancellation
/// + errors come back as `{ success: false, cancelled?, error? }`.
#[no_mangle]
pub extern "C" fn js_google_auth_sign_in() -> *mut perry_ffi::Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    platform::sign_in(promise);
    raw
}

/// `js_google_auth_silent_sign_in()` — restore a previous sign-in
/// without user interaction. Resolves to the same shape as
/// [`js_google_auth_sign_in`]; reports `success: false` if no
/// cached session exists.
#[no_mangle]
pub extern "C" fn js_google_auth_silent_sign_in() -> *mut perry_ffi::Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    platform::silent_sign_in(promise);
    raw
}

/// `js_google_auth_sign_out()` — clear cached credentials. The
/// resolved JSON is `{ success: true, ... }` on platforms where
/// the SDK reports a successful sign-out, otherwise the
/// `{ success: false, error }` shape.
#[no_mangle]
pub extern "C" fn js_google_auth_sign_out() -> *mut perry_ffi::Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    platform::sign_out(promise);
    raw
}

// No `#[cfg(test)] mod tests` block here: the FFI entry points
// allocate through `perry_ffi_promise_new`, a symbol provided by
// perry-stdlib at final-binary link time. A standalone
// `cargo test -p perry-ext-google-auth` (no stdlib in the link
// graph) would fail to link, matching every other promise-based
// `perry-ext-*` wrapper (bcrypt, argon2, fetch, …) which all have
// zero unit tests for the same reason. The smoke test under
// `test-files/test_google_auth_compile.ts` exercises the surface
// end-to-end against the linked binary.
