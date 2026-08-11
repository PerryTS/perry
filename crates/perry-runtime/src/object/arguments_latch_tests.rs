//! The `#7854` arguments-registry emptiness latch.
//!
//! `is_arguments_object` is a PROBE: it runs on paths that have nothing to do
//! with `arguments` — the by-name property-get tail, `Array.prototype.push`,
//! the array / `Symbol.iterator` iterator entries, `Array.from` / `concat`,
//! class construction. Before the latch every one of those paid a thread-local
//! resolution plus a `RefCell` borrow plus a pointer hash to prove the absence
//! of a feature most programs never use (2.8% of `gc-handoff/apps/interp.ts`).
//!
//! The whole soundness argument is "`ARGUMENTS_OBJECTS` has exactly ONE insert
//! site, and it arms the latch before inserting". These tests pin both halves,
//! and they pin THE SUBJECT rather than the answer: `latch_off_is_what_makes
//! _the_probe_cheap` forces the latch off with a real entry present and requires
//! the probe to answer `false`, so a deleted or never-taken short-circuit turns
//! that test red instead of leaving it quietly green. (A test that only asserted
//! `is_arguments_object(args) == true` would pass with the latch deleted — case
//! 4 of CLAUDE.md's "four ways a gate can be unable to fail".)
//!
//! The latch is process-global on purpose (Darwin has no local-exec TLS, so a
//! `thread_local!` flag would cost the very `_tlv_get_addr` this removes), which
//! makes it only ever CONSERVATIVE: one thread arming it sends every thread back
//! to the registry, i.e. to the pre-#7854 behaviour.

use super::*;

fn plain_object() -> *mut ObjectHeader {
    js_object_alloc(0, 4)
}

fn args_object() -> *mut ObjectHeader {
    let arr = crate::array::js_array_alloc(2);
    let arr = crate::array::js_array_push_f64(arr, 1.0);
    let arr = crate::array::js_array_push_f64(arr, 2.0);
    let raw_args = crate::value::js_nanbox_pointer(arr as i64);
    let callee = f64::from_bits(crate::value::TAG_UNDEFINED);
    js_arguments_object_alloc(raw_args, callee, 0)
}

/// The load-bearing claim: creating an arguments object arms the latch. If a
/// future edit adds a second `ARGUMENTS_OBJECTS` insert site without arming it,
/// `is_arguments_object` starts answering `false` for a real arguments object —
/// a silent wrong answer — and this goes red.
#[test]
fn creating_an_arguments_object_arms_the_latch() {
    let args = args_object();
    assert!(
        !crate::object::arguments::test_arguments_registry_never_used(),
        "js_arguments_object_alloc must arm ARGUMENTS_OBJECTS_EVER_USED before inserting"
    );
    assert!(
        crate::object::is_arguments_object(args),
        "a real arguments object must still be recognised once the latch is armed"
    );
    assert!(
        !crate::object::is_arguments_object(plain_object()),
        "an ordinary object must not be recognised as an arguments object"
    );
    assert!(
        !crate::object::is_arguments_object(std::ptr::null()),
        "a null receiver must answer false"
    );
}

/// Sabotage: with a REAL arguments object registered, force the latch back off
/// and require the probe to answer `false`. That answer is wrong — deliberately
/// — and it is the proof that the short-circuit is the arm being taken, not
/// dead code sitting in front of a registry lookup that would have answered
/// anyway. Deleting the `arguments_registry_never_used()` early-out makes this
/// test fail.
///
/// Also asserts the probe was ENTERED (`TEST_ARGUMENTS_REGISTRY_PROBES` moves),
/// so a future refactor that inlines the call away cannot leave this vacuous.
#[test]
fn latch_off_is_what_makes_the_probe_cheap() {
    let args = args_object();
    assert!(crate::object::is_arguments_object(args));

    let restore = !crate::object::arguments::test_arguments_registry_never_used();
    crate::object::arguments::test_force_arguments_registry_ever_used(false);
    let before = crate::object::arguments::test_arguments_registry_probe_count();
    let answered = crate::object::is_arguments_object(args);
    let probes_moved = crate::object::arguments::test_arguments_registry_probe_count() > before;
    crate::object::arguments::test_force_arguments_registry_ever_used(restore);

    assert!(probes_moved, "the probe must have been entered");
    assert!(
        !answered,
        "with the latch off the probe must short-circuit before the registry — \
         a `true` here means the early-out is gone and every non-`arguments` \
         program is paying the thread-local + hash again"
    );
    assert!(
        crate::object::is_arguments_object(args),
        "restoring the latch must restore the correct answer"
    );
}
