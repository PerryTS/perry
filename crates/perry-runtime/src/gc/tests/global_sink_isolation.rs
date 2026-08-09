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
        "{label}: the probe installed nothing, so the survived-vs-wiped distinction \
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
    canary().lock().unwrap_or_else(|p| p.into_inner()).clear();
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
                && crate::closure::closure_static_prototype(owner) == Some(0x7FFD_0000_0000_7672)
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

/// `symbol::test_clear_symbol_side_table_roots` — `SYMBOL_PROPERTIES`,
/// `SYMBOL_PROPERTY_ATTRS`, `CLASS_STATIC_SYMBOLS`, `SYMBOL_ACCESSOR_PROPERTIES`
/// and the `SYMBOL_POINTERS` rebuild.
///
/// The largest reader cluster in the survey behind #7672: 14 tests populate one
/// of these and assert on it without taking the clearing lock.
#[test]
fn symbol_side_tables_survive_a_guard_clear_on_another_thread() {
    let owner = 0x_5B01_0000_7672_usize;
    let sym_key = 0x_5B01_0000_7673_usize;
    let class_id = 0x7672_u32;
    let survived = survives_a_foreign_clear(
        "symbol side tables",
        || {
            crate::symbol::test_seed_symbol_property_root(owner, sym_key, 0x7FFD_0000_0000_7672);
            crate::symbol::test_seed_class_static_symbol_root(
                class_id,
                sym_key,
                0x7FFD_0000_0000_7673,
            );
            crate::symbol::test_seed_symbol_pointer_root(sym_key);
        },
        move || {
            crate::symbol::test_symbol_property_root_bits(owner, sym_key)
                == Some(0x7FFD_0000_0000_7672)
                && crate::symbol::test_class_static_symbol_root_bits(class_id, sym_key)
                    == Some(0x7FFD_0000_0000_7673)
                && crate::symbol::test_symbol_pointer_root_contains(sym_key)
        },
        foreign_guard_clear,
    );
    assert!(
        survived,
        "a symbol side-table entry written on this thread was destroyed by the GC \
         test guards' state reset running on another thread (#7672)"
    );
}

/// `timer::test_clear_all_timer_scanner_roots` — `TIMER_QUEUE`,
/// `CALLBACK_TIMERS`, `INTERVAL_TIMERS`.
#[test]
fn timer_queues_survive_a_guard_clear_on_another_thread() {
    let survived = survives_a_foreign_clear(
        "timer queues",
        || crate::timer::test_seed_many_timeout_roots(&[f64::from_bits(0x7FFD_0000_0000_7672)]),
        || crate::timer::test_timer_scanner_snapshot().timeout_value_bits == 0x7FFD_0000_0000_7672,
        foreign_guard_clear,
    );
    assert!(
        survived,
        "a queued timer written on this thread was destroyed by the GC test guards' \
         state reset running on another thread (#7672)"
    );
}

/// `object::test_clear_class_side_table_roots` — the class-registry `RwLock`
/// tables (`CLASS_PROTOTYPE_OBJECTS`, `CLASS_PARENT_CLOSURES`, …), which the
/// guard sets to `None` wholesale rather than per class id.
#[test]
fn class_registry_tables_survive_a_guard_clear_on_another_thread() {
    let proto_cid = 0x7672_u32;
    let closure_cid = 0x7673_u32;
    let survived = survives_a_foreign_clear(
        "class registry",
        || {
            crate::object::test_seed_class_inheritance_roots(proto_cid, 0x7672_0000);
            crate::object::test_seed_class_parent_closure_root(closure_cid, 0x7673_0000);
        },
        move || {
            crate::object::test_class_prototype_object_root(proto_cid) == 0x7672_0000
                && crate::object::test_class_parent_closure_root(closure_cid) == 0x7673_0000
        },
        foreign_guard_clear,
    );
    assert!(
        survived,
        "a class-registry entry written on this thread was destroyed by the GC test \
         guards' state reset running on another thread (#7672)"
    );
}

/// `object::test_clear_object_cache_roots` — the `AtomicU64` module-constant
/// caches and `GLOBAL_THIS_PTR`. These hold pointers into the *allocating
/// thread's* arena, so per-thread storage is also the only sound shape for
/// them; `array/tests.rs` carries 256-iteration retry loops written around
/// exactly this race.
#[test]
fn object_cache_roots_survive_a_guard_clear_on_another_thread() {
    let bits: [u64; 7] = [
        0x7FFD_0000_0000_7670,
        0x7FFD_0000_0000_7671,
        0x7FFD_0000_0000_7672,
        0x7FFD_0000_0000_7673,
        0x7FFD_0000_0000_7674,
        0x7FFD_0000_0000_7675,
        0x7FFD_0000_0000_7676,
    ];
    let survived = survives_a_foreign_clear(
        "object caches",
        || crate::object::test_seed_object_cache_roots(bits, 0x7672_0000),
        move || crate::object::test_object_cache_roots() == (bits, 0x7672_0000),
        foreign_guard_clear,
    );
    assert!(
        survived,
        "an object-cache root written on this thread was destroyed by the GC test \
         guards' state reset running on another thread (#7672)"
    );
}

/// `geisterhand_registry::test_clear_geisterhand_roots` — `REGISTRY` and
/// `PENDING_ACTIONS`.
#[test]
fn geisterhand_registry_survives_a_guard_clear_on_another_thread() {
    let closure = f64::from_bits(0x7FFD_0000_0000_7672);
    let survived = survives_a_foreign_clear(
        "geisterhand registry",
        || crate::geisterhand_registry::test_seed_geisterhand_roots(closure, 1.0, 2.0),
        move || {
            crate::geisterhand_registry::test_geisterhand_roots_snapshot().0
                == 0x7FFD_0000_0000_7672
        },
        foreign_guard_clear,
    );
    assert!(
        survived,
        "a geisterhand registry entry written on this thread was destroyed by the GC \
         test guards' state reset running on another thread (#7672)"
    );
}

/// `ui_text_registry::test_clear_ui_text_registry_roots` — `STATE_VALUES` and
/// `FOREACH_REGISTRY`.
#[test]
fn ui_text_registry_survives_a_guard_clear_on_another_thread() {
    let state = f64::from_bits(0x7FFD_0000_0000_7672);
    let render = f64::from_bits(0x7FFD_0000_0000_7673);
    let survived = survives_a_foreign_clear(
        "ui text registry",
        || crate::ui_text_registry::test_seed_ui_text_registry_roots(state, render),
        move || {
            crate::ui_text_registry::test_ui_text_registry_roots_snapshot()
                == (0x7FFD_0000_0000_7672, 0x7FFD_0000_0000_7673)
        },
        foreign_guard_clear,
    );
    assert!(
        survived,
        "a UI text registry entry written on this thread was destroyed by the GC test \
         guards' state reset running on another thread (#7672)"
    );
}

// ---------------------------------------------------------------------------
// The probe list may not rot: every `clear_helper` named here must still be
// called by the guards' reset, and the reset's source is the authority.
// ---------------------------------------------------------------------------

/// Every `test_clear_*` helper this file claims to cover.
const COVERED_CLEAR_HELPERS: &[&str] = &[
    "test_clear_closure_side_tables",
    "test_clear_symbol_side_table_roots",
    "test_clear_all_timer_scanner_roots",
    "test_clear_class_side_table_roots",
    "test_clear_object_cache_roots",
    "test_clear_geisterhand_roots",
    "test_clear_ui_text_registry_roots",
];

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
