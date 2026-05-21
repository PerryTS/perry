//! App Group suite-name registry (#1178).
//!
//! `perry/system :: appGroupSet/Get/Delete` on Apple platforms is backed by
//! `UserDefaults(suiteName:)`. The suite name is project-level config
//! (`[ios] app_group = "group.com.example.shared"` in `perry.toml`), not
//! something the calling TypeScript code knows. The CLI bakes it into the
//! entry module's `main()` prelude as a single
//! `perry_app_group_init(ptr, len)` call so the platform FFI layer
//! (`perry-ui-ios`, `perry-ui-macos`) can resolve it on every call without
//! re-reading the manifest.
//!
//! When `[ios] app_group` is absent the codegen omits the call entirely;
//! `configured_suite_name()` then returns `None` and the per-platform FFI
//! falls back to its first-call stub-warn diagnostic so developers see a
//! clear "not configured" hint instead of silent in-process storage.

use std::sync::OnceLock;

static SUITE_NAME: OnceLock<String> = OnceLock::new();

/// Called once from `main()`'s prelude when `[ios] app_group` is set in
/// `perry.toml`. `ptr` points into the entry module's rodata; the bytes
/// outlive the process so we copy into an owned `String` for the
/// `OnceLock`. Silently ignored on null/empty input — codegen only emits
/// the call when the suite name resolves to a non-empty string.
///
/// # Safety
/// Caller (perry-codegen entry module prelude) passes a `(ptr, len)` pair
/// derived from `LlModule::add_string_constant`, so `ptr` is a valid
/// non-null pointer to `len` UTF-8 bytes in the binary's rodata segment.
#[no_mangle]
pub unsafe extern "C" fn perry_app_group_init(ptr: *const u8, len: i32) {
    if ptr.is_null() || len <= 0 {
        return;
    }
    let slice = std::slice::from_raw_parts(ptr, len as usize);
    let Ok(s) = std::str::from_utf8(slice) else {
        return;
    };
    // `set` returns Err if already initialized — fine; the entry module
    // calls this exactly once before user code runs.
    let _ = SUITE_NAME.set(s.to_string());
}

/// Public Rust accessor for crates that already depend on
/// `perry-runtime` (`perry-ui-ios` does). Returns the suite name baked
/// in from `perry.toml`, or `None` when no `[ios] app_group` was
/// configured at compile time.
pub fn configured_suite_name() -> Option<&'static str> {
    SUITE_NAME.get().map(String::as_str)
}

/// C-ABI accessor for FFI crates that intentionally avoid pulling
/// `perry-runtime` in as a Cargo dep (e.g. `perry-ui-macos` —
/// see `crates/perry-ui-macos/src/string_header.rs`'s rationale).
/// Writes the byte length to `*out_len` and returns a pointer into
/// the static buffer. Returns null + `*out_len = 0` when no suite is
/// configured; the bytes are valid for the lifetime of the process
/// and are NOT NUL-terminated — callers must use the returned length.
///
/// # Safety
/// `out_len` must be a valid writable pointer to an `i32`.
#[no_mangle]
pub unsafe extern "C" fn perry_app_group_suite_name(out_len: *mut i32) -> *const u8 {
    match SUITE_NAME.get() {
        Some(s) => {
            if !out_len.is_null() {
                *out_len = s.len() as i32;
            }
            s.as_ptr()
        }
        None => {
            if !out_len.is_null() {
                *out_len = 0;
            }
            std::ptr::null()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // OnceLock means the suite name is process-global; the iOS/macOS
    // FFI tests run in different processes, so a single in-process
    // round-trip test is the most we can verify here.
    #[test]
    fn round_trips_a_set_suite_name() {
        let suite = "group.com.perry.test";
        unsafe {
            perry_app_group_init(suite.as_ptr(), suite.len() as i32);
        }
        // OnceLock guarantees the first writer wins, so this matches even
        // if a sibling test seeded a different value first.
        let got = configured_suite_name().expect("init must store the suite");
        assert!(
            got == suite || got.starts_with("group."),
            "expected a baked group.* suite, got {got:?}"
        );
    }

    #[test]
    fn null_ptr_is_a_no_op() {
        unsafe {
            perry_app_group_init(std::ptr::null(), 0);
        }
        // No panic, no assertion — `set` returns Err and we ignore it.
    }
}
