//! TEMPORARY (shapes campaign): per-precondition miss counters for the
//! class-field IC, so a widened guard can be shown to be TAKEN rather than
//! merely EMITTED.
//!
//! Off unless `PERRY_CLASS_FIELD_COUNTERS=1`. Reverted before the PR.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const N: usize = 12;
pub static C: [AtomicU64; N] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

pub const NAMES: [&str; N] = [
    "set_guard_calls",     // 0: runtime set guard entered (= inline precheck MISS)
    "set_guard_pass",      // 1: ... and the runtime contract accepted
    "get_guard_calls",     // 2
    "get_guard_pass",      // 3
    "put_value_set_calls", // 4: sloppy-arm inline precheck MISS
    "contract_notobj",     // 5: fast_contract: not a heap object / bad header
    "contract_cid",        // 6: fast_contract: class_id mismatch
    "contract_keys",       // 7: fast_contract: keys_array mismatch
    "contract_fieldcount", // 8: fast_contract: field_index >= field_count
    "contract_rawf64",     // 9: fast_contract: side table says slot is not raw-f64
    "set_frozen",          // 10: set contract: frozen
    "set_notplain",        // 11: set contract: value is not a plain number (INT32-boxed etc.)
];

static ENABLED: AtomicBool = AtomicBool::new(false);
static INIT: std::sync::Once = std::sync::Once::new();

#[inline]
pub fn enabled() -> bool {
    INIT.call_once(|| {
        let on = std::env::var("PERRY_CLASS_FIELD_COUNTERS")
            .map(|v| matches!(v.trim(), "1" | "true" | "on" | "yes"))
            .unwrap_or(false);
        ENABLED.store(on, Ordering::Relaxed);
        if on {
            extern "C" fn report() {
                eprint!("[class-field]");
                for i in 0..N {
                    eprint!(" {}={}", NAMES[i], C[i].load(Ordering::Relaxed));
                }
                eprintln!();
            }
            unsafe {
                libc::atexit(report);
            }
        }
    });
    ENABLED.load(Ordering::Relaxed)
}

#[inline]
pub fn bump(i: usize) {
    if enabled() {
        C[i].fetch_add(1, Ordering::Relaxed);
    }
}
