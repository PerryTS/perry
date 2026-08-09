//! #7672: the GC test guards clear process-global side tables from whatever
//! libtest thread runs them, and no reader is required to take the clearing
//! lock.
//!
//! Each probe below plants the shape that produced the three flakes of
//! 2026-08-08/09 — write on THIS thread, run the guards' clear on ANOTHER
//! thread, read back — and asserts the entry survived. That is only meaningful
//! because the same harness is shown to catch a table that has *not* been
//! converted: `the_probe_catches_a_bare_process_global_sink` runs a canary
//! declared as a plain `static` and requires the wipe to be observed. A green
//! file therefore means the detector works, not that nothing was tried.
//!
//! The probes deliberately do NOT take `crate::gc::global_side_table_test_lock()`.
//! Taking it is what the ~180 unconverted readers are not required to do, and a
//! probe that took it would be testing the opt-in rather than the isolation.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Run one probe: install on this thread, clear on another, observe here.
///
/// Returns `true` if the installed entry survived the foreign clear.
///
/// Panics if `install` did not make `observe` true to begin with — a probe
/// whose subject was never live cannot distinguish "survived" from "was never
/// there", which is exactly the vacuity this file exists to avoid.
fn survives_a_foreign_clear(
    label: &str,
    install: impl FnOnce(),
    observe: impl Fn() -> bool,
    clear: fn(),
) -> bool {
    install();
    assert!(
        observe(),
        "{label}: the probe installed nothing, so the surviving/​wiped distinction \
         would be vacuous — fix the probe before reading its verdict"
    );
    std::thread::spawn(clear)
        .join()
        .expect("the clearing thread panicked");
    observe()
}

/// The guards' own state reset, run from another thread — verbatim what
/// `GcTestIsolationGuard::new` and `CopyingNurseryTestGuard::new/drop` do.
fn foreign_guard_clear() {
    super::support::reset_copying_nursery_runtime_test_state();
}

// ---------------------------------------------------------------------------
// The canary: a bare process-global sink, and proof the harness catches it.
// ---------------------------------------------------------------------------

/// Deliberately NOT `guard_cleared_global!`. This is the pre-#7672 shape of
/// every table in this file, kept so the probe above is proven able to fail.
static CANARY_SINK: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();

fn canary() -> &'static Mutex<HashMap<usize, u64>> {
    CANARY_SINK.get_or_init(|| Mutex::new(HashMap::new()))
}

fn canary_clear() {
    canary()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
}

#[test]
fn the_probe_catches_a_bare_process_global_sink() {
    let key = 0xCA_1A_0000_7672usize;
    let survived = survives_a_foreign_clear(
        "canary",
        || {
            canary()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(key, 0x7672);
        },
        move || {
            canary()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&key)
                .copied()
                == Some(0x7672)
        },
        canary_clear,
    );
    assert!(
        !survived,
        "the isolation probe reported a BARE process-global sink as surviving a \
         clear on another thread. The probe can no longer fail, so every other \
         test in this file is vacuous — fix the probe, do not delete this test."
    );
}

// ---------------------------------------------------------------------------
// Converted tables. One test per `test_clear_*` helper the guards call.
// ---------------------------------------------------------------------------

/// `closure::test_clear_closure_side_tables` — `CLOSURE_PROPS`,
/// `CLOSURE_STATIC_PROTOTYPES`, `CLOSURE_DELETED_KEYS`.
///
/// This is the #7671 shape exactly: a value written through the closure
/// dynamic-property table and read back came out `TAG_UNDEFINED` because a GC
/// guard on another libtest thread had emptied the table in between.
#[test]
fn closure_side_tables_survive_a_guard_clear_on_another_thread() {
    // A synthetic, per-test key: the tables are keyed by heap address but the
    // accessors treat the key as an opaque integer for store/load.
    let owner = 0x_C105_0000_7672_usize;
    let survived = survives_a_foreign_clear(
        "closure dynamic props",
        || {
            crate::closure::closure_set_dynamic_prop(owner, "probe7672", 42.5);
            crate::closure::closure_set_static_prototype(owner, 0x7FFD_0000_0000_7672);
            crate::closure::closure_mark_key_deleted(owner, "goneKey");
        },
        move || {
            crate::closure::closure_get_own_dynamic_prop(owner, "probe7672") == Some(42.5)
                && crate::closure::closure_has_own_dynamic_prop(owner, "probe7672")
                && crate::closure::closure_static_prototype(owner)
                    == Some(0x7FFD_0000_0000_7672)
                && crate::closure::closure_is_key_deleted(owner, "goneKey")
        },
        foreign_guard_clear,
    );
    assert!(
        survived,
        "a closure dynamic property written on this thread was destroyed by the GC \
         test guards' state reset running on another thread (#7672 / #7671). The \
         table's `guard_cleared_global!` declaration is what prevents this."
    );
}

// ---------------------------------------------------------------------------
// The probe list may not rot: every `clear_helper` named here must still be
// called by the guards' reset, and the reset's source is the authority.
// ---------------------------------------------------------------------------

/// Every `test_clear_*` helper this file claims to cover.
const COVERED_CLEAR_HELPERS: &[&str] = &["test_clear_closure_side_tables"];

#[test]
fn every_covered_clear_helper_is_still_called_by_the_guards() {
    // Compile-time include: a probe for a helper that no longer runs proves
    // nothing, and would sit green forever.
    let support = include_str!("support.rs");
    let body = support
        .split_once("fn reset_copying_nursery_runtime_test_state()")
        .expect("the guards' reset function was renamed — update this test")
        .1;
    let body = body.split_once("\n}\n").expect("unterminated reset fn").0;
    assert!(
        body.contains("test_clear_closure_side_tables"),
        "sanity: the extracted reset body does not contain a call it certainly \
         makes, so the extraction is broken and this check is vacuous"
    );
    for helper in COVERED_CLEAR_HELPERS {
        assert!(
            body.contains(helper),
            "{helper} is probed in this file but is no longer called by \
             reset_copying_nursery_runtime_test_state — the probe has rotted"
        );
    }
}
