//! Runtime view of `perry compile --platform bun` (#10360).
//!
//! The platform is a compile-time choice, but a few Web APIs differ between
//! Node and Bun at runtime. Under `--platform bun` the compiler seeds every
//! module's init with `__perry_runtime.setBunPlatform()` (next to the
//! `globalThis.Bun` install), so the flag is set before any user code runs.
//! It is process-global and only ever turns on.
//!
//! Both setter and getter are `#[no_mangle]` so perry-stdlib and the ext
//! crates (which link the runtime by symbol, not by Rust path) all read the
//! same flag.

use std::sync::atomic::{AtomicBool, Ordering};

static BUN_PLATFORM: AtomicBool = AtomicBool::new(false);

/// Called from generated module init under `--platform bun`.
#[no_mangle]
pub extern "C" fn js_set_bun_platform() {
    BUN_PLATFORM.store(true, Ordering::Relaxed);
}

/// 1 when the program was compiled with `--platform bun`, else 0.
#[no_mangle]
pub extern "C" fn js_bun_platform_enabled() -> i32 {
    i32::from(BUN_PLATFORM.load(Ordering::Relaxed))
}

#[cfg(test)]
pub(crate) fn reset_bun_platform_for_test() {
    BUN_PLATFORM.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bun_platform_flag_defaults_off_and_turns_on() {
        reset_bun_platform_for_test();
        assert_eq!(js_bun_platform_enabled(), 0);
        js_set_bun_platform();
        assert_eq!(js_bun_platform_enabled(), 1);
        reset_bun_platform_for_test();
        assert_eq!(js_bun_platform_enabled(), 0);
    }
}
