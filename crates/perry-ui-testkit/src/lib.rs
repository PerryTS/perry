//! Shared UI test-mode plumbing used by every perry-ui-* backend.
//!
//! When `PERRY_UI_TEST_MODE=1` is set, UI backends schedule an early exit
//! after rendering a frame. Optional `PERRY_UI_SCREENSHOT_PATH` writes a PNG
//! of the main window before exiting. This exists so documentation-example
//! programs can be compiled and launched in CI without a human.

pub const ENV_TEST_MODE: &str = "PERRY_UI_TEST_MODE";
pub const ENV_EXIT_DELAY_MS: &str = "PERRY_UI_TEST_EXIT_AFTER_MS";
pub const ENV_SCREENSHOT_PATH: &str = "PERRY_UI_SCREENSHOT_PATH";

const DEFAULT_EXIT_DELAY_MS: u32 = 200;

pub fn is_test_mode() -> bool {
    match std::env::var(ENV_TEST_MODE) {
        Ok(v) => {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
        }
        Err(_) => false,
    }
}

pub fn exit_delay_ms() -> u32 {
    std::env::var(ENV_EXIT_DELAY_MS)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(DEFAULT_EXIT_DELAY_MS)
}

pub fn screenshot_path() -> Option<String> {
    std::env::var(ENV_SCREENSHOT_PATH)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Write a malloc'd PNG buffer (as returned by `perry_ui_screenshot_capture`)
/// to the path from `PERRY_UI_SCREENSHOT_PATH`. Caller retains ownership of
/// the buffer; this function reads and ignores it on failure.
///
/// Safe to call on any platform — no-op if `ptr.is_null()` or `len == 0`.
pub fn write_screenshot_bytes(path: &str, ptr: *const u8, len: usize) -> bool {
    if ptr.is_null() || len == 0 {
        return false;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::fs::write(path, slice).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env-mutation tests. cargo test runs each `#[test]` in
    // parallel by default (one thread per CPU), and `std::env::set_var` /
    // `remove_var` are process-global — two tests touching the same env
    // var at once will see each other's mutations and race their
    // assertions. macos-14's CI runner has 3 vCPUs which made this rare;
    // the ubuntu-latest migration in v0.5.392 bumped that to 4 vCPUs and
    // surfaced `test_mode_off_for_falsy_values` as a flake. The Mutex
    // serializes EVERY env-mutating test in this mod regardless of which
    // var it touches, which is overkill but cheap (each test runs in
    // microseconds) and prevents future env-var collisions.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        // Hold the lock for the entire setup → run → restore window so
        // concurrent tests can't observe partial state. PoisonError ignored
        // — a panicking test still leaves a usable lock for others.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        f();
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn test_mode_off_by_default() {
        with_env(ENV_TEST_MODE, None, || {
            assert!(!is_test_mode());
        });
    }

    #[test]
    fn test_mode_on_for_truthy_values() {
        with_env(ENV_TEST_MODE, Some("1"), || assert!(is_test_mode()));
        with_env(ENV_TEST_MODE, Some("true"), || assert!(is_test_mode()));
        with_env(ENV_TEST_MODE, Some("yes"), || assert!(is_test_mode()));
    }

    #[test]
    fn test_mode_off_for_falsy_values() {
        with_env(ENV_TEST_MODE, Some(""), || assert!(!is_test_mode()));
        with_env(ENV_TEST_MODE, Some("0"), || assert!(!is_test_mode()));
        with_env(ENV_TEST_MODE, Some("false"), || assert!(!is_test_mode()));
        with_env(ENV_TEST_MODE, Some("FALSE"), || assert!(!is_test_mode()));
    }

    #[test]
    fn exit_delay_default_and_override() {
        with_env(ENV_EXIT_DELAY_MS, None, || {
            assert_eq!(exit_delay_ms(), DEFAULT_EXIT_DELAY_MS);
        });
        with_env(ENV_EXIT_DELAY_MS, Some("750"), || {
            assert_eq!(exit_delay_ms(), 750);
        });
        with_env(ENV_EXIT_DELAY_MS, Some("garbage"), || {
            assert_eq!(exit_delay_ms(), DEFAULT_EXIT_DELAY_MS);
        });
    }

    #[test]
    fn screenshot_path_trims_and_rejects_empty() {
        with_env(ENV_SCREENSHOT_PATH, None, || {
            assert_eq!(screenshot_path(), None);
        });
        with_env(ENV_SCREENSHOT_PATH, Some("  "), || {
            assert_eq!(screenshot_path(), None);
        });
        with_env(ENV_SCREENSHOT_PATH, Some("/tmp/foo.png"), || {
            assert_eq!(screenshot_path().as_deref(), Some("/tmp/foo.png"));
        });
    }
}
