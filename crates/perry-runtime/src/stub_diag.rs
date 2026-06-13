//! First-call runtime diagnostic for no-op stubs (issue #464).
//!
//! Some FFI symbols in perry-runtime are intentional no-ops — for the
//! harmonyos build the entire `perry_ui_*` / `perry_system_*` /
//! `perry_updater_*` surface is auto-generated as zero-returning stubs
//! by `build.rs` (issue #395 / #399), and the runtime-only build has
//! a handful of `js_ws_*` / `js_readline_*` stubs in `stdlib_stubs.rs`
//! for the case where `perry-stdlib` isn't linked.
//!
//! Without a signal these stubs reproduce the exact DX cliff #455
//! describes: the program runs, produces wrong output, no warning. So
//! every stub funnels through [`perry_stub_warn`] which prints a
//! single `[perry] warning: ...` line to stderr the first time each
//! symbol is invoked. Subsequent calls stay silent so a hot-loop
//! doesn't flood the terminal.
//!
//! ## `PERRY_STUB_DIAG` env var
//!
//! - unset / `auto` / `default` — first call per symbol prints once.
//! - `off` / `0` / `false` / `silent` — silenced entirely.
//! - `verbose` / `all` — every call prints (debugging aid).
//!
//! The mode is read once on first warning and cached.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DiagMode {
    Off,
    FirstCall,
    Verbose,
}

fn mode() -> DiagMode {
    static MODE: OnceLock<DiagMode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("PERRY_STUB_DIAG").as_deref() {
        Ok("off") | Ok("0") | Ok("false") | Ok("silent") => DiagMode::Off,
        Ok("verbose") | Ok("all") | Ok("every") => DiagMode::Verbose,
        _ => DiagMode::FirstCall,
    })
}

fn seen() -> &'static Mutex<HashSet<&'static str>> {
    static SEEN: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Print a single first-call warning for a stubbed FFI symbol.
///
/// `name` is the runtime symbol (e.g. `"perry_system_keychain_save"`).
/// `reason` is a one-liner explaining why this is a stub. `issue` is
/// an optional GitHub issue tag like `"#399"` so users can find the
/// tracking thread.
pub fn perry_stub_warn(name: &'static str, reason: &'static str, issue: Option<&'static str>) {
    let m = mode();
    if m == DiagMode::Off {
        return;
    }
    if m == DiagMode::FirstCall {
        let mut s = match seen().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if !s.insert(name) {
            return;
        }
    }
    match issue {
        Some(tag) => eprintln!(
            "[perry] warning: `{}` is a no-op stub on this platform — {} (tracking: {})",
            name, reason, tag
        ),
        None => eprintln!(
            "[perry] warning: `{}` is a no-op stub on this platform — {}",
            name, reason
        ),
    }
}

/// Whether `PERRY_STRICT_STUBS=1` (or `on`/`true`) is set — the opt-in
/// strictness mode from the stub-elimination epic (#4918/#4919). When
/// on, a runtime stub that would otherwise return a fake/no-op result
/// instead throws `ERR_PERRY_UNIMPLEMENTED`, turning silent lies into
/// measurable failures for package-compat work. Read once and cached.
pub fn strict_stubs_enabled() -> bool {
    static STRICT: OnceLock<bool> = OnceLock::new();
    *STRICT.get_or_init(|| {
        matches!(
            std::env::var("PERRY_STRICT_STUBS").as_deref(),
            Ok("1") | Ok("on") | Ok("true") | Ok("yes")
        )
    })
}

/// Whether `PERRY_DETERMINISTIC_NET=1` (or `on`/`true`/`yes`) is set — the
/// parity-CI mode for the now-real `node:dns` / `node:dgram` network stack
/// (#4911). When on, `dns` resolution and `dgram` sockets fall back to the
/// in-process deterministic loopback answers (no external name resolution,
/// no host UDP sockets) so byte-for-byte parity fixtures stay reproducible
/// on machines without network access. Unset (the default), the modules do
/// real `getaddrinfo`/DNS/UDP I/O. Read once and cached.
pub fn deterministic_net_enabled() -> bool {
    static DETERMINISTIC: OnceLock<bool> = OnceLock::new();
    *DETERMINISTIC.get_or_init(|| {
        matches!(
            std::env::var("PERRY_DETERMINISTIC_NET").as_deref(),
            Ok("1") | Ok("on") | Ok("true") | Ok("yes")
        )
    })
}

/// First-call diagnostic for a *runtime* API stub (dns/dgram loopback
/// fakes, v8 heap snapshots, stdlib-adapter no-ops, …) — the
/// manifest-flagged clusters from the
/// stub-elimination epic (#4919). Unlike [`perry_stub_warn`], whose
/// message says "on this platform" (for the harmonyos UI stubs), this
/// says the API itself is registered-but-fake regardless of target.
///
/// `name` is the JS-facing API (e.g. `"dns.resolve4"`), `reason` a
/// one-liner, `issue` the tracking tag. Honors the same `PERRY_STUB_DIAG`
/// modes (first-call / off / verbose) as [`perry_stub_warn`].
pub fn perry_runtime_stub(name: &'static str, reason: &'static str, issue: Option<&'static str>) {
    let m = mode();
    if m == DiagMode::Off {
        return;
    }
    if m == DiagMode::FirstCall {
        let mut s = match seen().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if !s.insert(name) {
            return;
        }
    }
    match issue {
        Some(tag) => eprintln!(
            "[perry] warning: `{}` is a stub (registered but not real) — {} (tracking: {})",
            name, reason, tag
        ),
        None => eprintln!(
            "[perry] warning: `{}` is a stub (registered but not real) — {}",
            name, reason
        ),
    }
}

/// C-ABI shim around [`perry_stub_warn`] for FFI crates that link
/// perry-runtime as a Cargo dependency but must reach it through a
/// stable, feature-set-independent symbol rather than the hash-mangled
/// Rust path `perry_runtime::stub_diag::perry_stub_warn`.
///
/// `perry-ui-ios` is the motivating caller (#1311): under the
/// `geisterhand` feature its UI staticlib is built against a
/// perry-runtime whose Cargo feature set can differ from the runtime
/// actually linked into the app, so the Rust-mangled symbol carries a
/// different `-Cmetadata` hash on each side and the link fails with
/// "Undefined symbols". A `#[no_mangle]` symbol has a fixed name on
/// both sides and resolves regardless of feature set — the same
/// discipline `perry_app_group_suite_name` already uses for the App
/// Group accessor (perry-ui-macos avoids the issue entirely by not
/// depending on perry-runtime).
///
/// `name`, `reason`, and (optional) `issue` are NUL-terminated C
/// strings that MUST point into static storage (string literals): the
/// dedup set keys on `name` as a `&'static str`.
///
/// # Safety
/// `name` and `reason` are non-null pointers to NUL-terminated UTF-8
/// living for the process lifetime; `issue` is either null or the same.
#[no_mangle]
pub unsafe extern "C" fn perry_stub_warn_ffi(
    name: *const std::os::raw::c_char,
    reason: *const std::os::raw::c_char,
    issue: *const std::os::raw::c_char,
) {
    if name.is_null() || reason.is_null() {
        return;
    }
    // SAFETY: callers pass `'static` C-string literals, so reborrowing
    // the slices for `'static` is sound.
    let as_static = |p: *const std::os::raw::c_char| -> &'static str {
        std::ffi::CStr::from_ptr(p).to_str().unwrap_or("")
    };
    let issue = if issue.is_null() {
        None
    } else {
        Some(as_static(issue))
    };
    perry_stub_warn(as_static(name), as_static(reason), issue);
}

/// One row of the auto-generated stub manifest.
///
/// Populated by `build.rs` from `perry-dispatch`'s tables plus the
/// hand-listed `direct_call_stubs`. Consumed by:
///  - the generated stub bodies (each calls `perry_stub_warn` with
///    matching args), and
///  - the `perry check` static scan, which walks user imports against
///    the `ts_name` column to surface stubs ahead of runtime.
#[derive(Copy, Clone, Debug)]
pub struct StubEntry {
    /// Runtime FFI symbol the stub body defines (`perry_ui_*`, …).
    pub symbol: &'static str,
    /// TypeScript-level name the user imports (`"keychainSave"`,
    /// `"notificationSchedule"`, …) when it can be derived from the
    /// dispatch tables. `None` for direct-call stubs whose TS shape
    /// is computed at codegen (e.g. trigger-variant fan-out).
    pub ts_name: Option<&'static str>,
    /// Source TS module that exposes `ts_name`
    /// (e.g. `"perry/system"`, `"perry/ui"`, `"perry/updater"`).
    pub module: &'static str,
    /// Why this symbol is a stub (shown in the warning).
    pub reason: &'static str,
    /// Tracking issue for the stub family (`"#399"`, etc.).
    pub issue: Option<&'static str>,
}

include!(concat!(env!("OUT_DIR"), "/perry_stub_manifest.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_constant_resolves() {
        // Smoke test that `STUB_MANIFEST` compiles and has the
        // expected shape — empty on non-harmonyos builds, populated
        // when `ohos-napi` is on. Either case is valid; this test
        // just exercises the include path.
        let _: &[StubEntry] = STUB_MANIFEST;
        for entry in STUB_MANIFEST {
            assert!(!entry.symbol.is_empty(), "stub manifest symbol empty");
            assert!(!entry.module.is_empty(), "stub manifest module empty");
            assert!(!entry.reason.is_empty(), "stub manifest reason empty");
        }
    }

    #[test]
    fn warn_does_not_panic_with_off_mode() {
        // Direct call shouldn't panic. We don't assert about output
        // here — the test runner captures stderr, and the OnceLock
        // mode cache means inter-test ordering matters. The dedicated
        // process-isolated tests below cover behavioural assertions.
        perry_stub_warn("test_smoke_symbol", "smoke test reason", Some("#464"));
    }
}
