use crate::exception::{catch_js_throw, current_try_depth, js_has_exception, js_throw};
use std::fmt::Debug;

/// Every registered subsystem must supply a witness that changes real state.
/// Test both nested landings with a nonempty outer scope, and run through the
/// production js_throw + C trampoline, never a test-only replay of cleanup.
pub(super) fn assert_nested_restore<T: Copy + Debug + PartialEq>(
    capture: fn() -> T,
    restore: fn(T),
    push: fn(u32),
) {
    struct Reset<T: Copy> {
        value: T,
        restore: fn(T),
    }
    impl<T: Copy> Drop for Reset<T> {
        fn drop(&mut self) {
            (self.restore)(self.value);
        }
    }

    let _lock = crate::gc::global_side_table_test_lock();
    let base = capture();
    // This guard lives outside every abandoned frame, including on a failing
    // assertion in the catch path. It also keeps injected faults isolated.
    let _reset = Reset {
        value: base,
        restore,
    };
    let try_base = current_try_depth();
    push(1);
    let outer = capture();
    assert_ne!(outer, base, "fixture must establish live outer state");
    let outcome: Result<(), f64> = catch_js_throw(|| {
        push(2);
        let inner = capture();
        assert_ne!(
            inner, outer,
            "fixture must change state inside the outer trap"
        );
        let nested: Result<(), f64> = catch_js_throw(|| {
            push(3);
            assert_ne!(
                capture(),
                inner,
                "fixture must change state inside the inner trap"
            );
            js_throw(31.0)
        });
        assert_eq!(nested, Err(31.0));
        assert_eq!(
            capture(),
            inner,
            "inner catch must preserve its enclosing scope"
        );
        assert_eq!(current_try_depth(), try_base + 1);
        js_throw(47.0)
    });
    assert_eq!(outcome, Err(47.0));
    assert_eq!(
        capture(),
        outer,
        "outer catch must preserve the preexisting scope"
    );
    assert_eq!(current_try_depth(), try_base);
    assert_eq!(js_has_exception(), 0);
    assert_eq!(catch_js_throw(|| push(4)), Ok(()));
    assert_ne!(
        capture(),
        outer,
        "normal completion must not replay cleanup"
    );
    assert_eq!(current_try_depth(), try_base);
}

pub(super) fn shadow(marker: u32) {
    crate::gc::js_shadow_frame_push(marker);
    crate::gc::js_shadow_slot_set(0, (marker as f64).to_bits());
    crate::gc::js_gc_temp_root_push((marker as f64).to_bits());
}

pub(super) fn runtime_handles(marker: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    scope.root_nanbox_f64(marker as f64);
    std::mem::forget(scope);
}

pub(super) fn call_method(_: u32) {
    crate::object::test_enter_catch_method();
}

pub(super) fn pump(_: u32) {
    crate::stdlib_pump::test_enter_catch_pump();
}

pub(super) fn set_foreach(_: u32) {
    crate::set::test_enter_catch_foreach();
}

pub(super) fn map_foreach(_: u32) {
    crate::map::test_enter_catch_foreach();
}

pub(super) fn prototype_resolution(_: u32) {
    let owner = crate::object::js_object_alloc(0, 0);
    assert!(crate::object::prototype_chain::test_resolution_stack_enter_and_forget(owner as usize));
}

pub(super) fn static_private_owner(marker: u32) {
    crate::object::static_private_owner_push(marker as f64);
}

pub(super) fn private_lexical_brand(marker: u32) {
    crate::object::private_lexical_brand_push(marker as f64);
}

pub(super) fn derived_super_binding(_: u32) {
    // The production stack stores native alloca addresses. No binding is
    // evaluated here, so an empty address suffices for the lifetime witness.
    crate::object::js_derived_super_scope_push(std::ptr::null_mut());
}

pub(super) fn private_member_access_hints(marker: u32) {
    crate::object::test_push_catch_private_hint(marker);
}

#[cfg(feature = "regex-engine")]
pub(super) fn regex_factory(marker: u32) {
    crate::regex::site_test::test_enter_catch_factory(marker);
}

#[cfg(feature = "dyn-eval")]
pub(super) fn dyn_eval(marker: u32) {
    crate::dyn_eval::root_push(marker as f64);
    crate::dyn_eval::call_depth_enter().unwrap();
}

/// The latch must go up on the first push, before any state exists that a
/// later capture could skip — and a set bit must switch capture back to the
/// real read. Sabotage caught: a `CatchStack::push` that forgets
/// `note_catch_subsystem_used` leaves the bit clear after the push below.
#[test]
fn catch_stack_push_latches_its_subsystem_bit() {
    use super::{catch_subsystem, catch_subsystem_used, CatchStack};
    // A bit no production subsystem uses, so this test owns it outright.
    const PRIVATE_BIT: u32 = 1 << 30;
    assert!(
        !catch_subsystem_used(PRIVATE_BIT),
        "fixture bit must start clear, or the latch assertion below is vacuous"
    );
    let mut stack = CatchStack::<u32>::new(PRIVATE_BIT);
    assert!(stack.is_empty());
    assert!(
        !catch_subsystem_used(PRIVATE_BIT),
        "construction must not latch"
    );
    stack.push(7);
    assert!(
        catch_subsystem_used(PRIVATE_BIT),
        "the first push must latch"
    );
    assert_eq!(stack.pop(), Some(7));
    assert!(catch_subsystem_used(PRIVATE_BIT), "bits are never cleared");
    assert!(catch_subsystem_used(catch_subsystem::ALWAYS));
}

/// A stack pushed on one thread must be captured exactly on another thread
/// that pushes later: the bit is process-wide, the depth is per thread.
#[test]
fn latched_capture_reads_real_depth_on_every_thread() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::object::static_private_owner_push(1.0);
    let here = crate::object::static_private_owner_stack_savepoint();
    assert!(here >= 1);
    let there = std::thread::spawn(|| {
        let before = crate::object::static_private_owner_stack_savepoint();
        crate::object::static_private_owner_push(2.0);
        crate::object::static_private_owner_push(3.0);
        let after = crate::object::static_private_owner_stack_savepoint();
        crate::object::static_private_owner_stack_restore(before);
        (before, after)
    })
    .join()
    .expect("worker thread");
    assert_eq!(there, (0, 2));
    crate::object::static_private_owner_stack_restore(here - 1);
}
