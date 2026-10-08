//! Forwarding with an explicit `this` (`call` / `apply`, bound functions,
//! `bind`) holds the callee, the receiver and every argument across the steps
//! that can collect: boxing the receiver, cloning a `this`-capturing method,
//! running a `name` getter, allocating the bound-arguments array.
//!
//! Each test arms the named collection point that stands for one of those
//! steps, so a copying minor runs exactly there, then asserts the callee saw
//! each young heap argument at its POST-collection address. Every test also
//! asserts its premise (a copying minor ran and the arguments moved): a
//! collection that moved nothing would pass vacuously.

use super::call_argument_lists::tenured_closure;
use super::*;
use std::cell::RefCell;

crate::perry_thread_local! {
    static SEEN: RefCell<Vec<u64>> = RefCell::new(Vec::new());
}

fn seen() -> Vec<u64> {
    SEEN.with(|seen| std::mem::take(&mut *seen.borrow_mut()))
}

extern "C" fn record_this_and_two(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    a: f64,
    b: f64,
) -> f64 {
    SEEN.with(|seen| *seen.borrow_mut() = vec![this.bits(), a.to_bits(), b.to_bits()]);
    0.0
}

fn addr(value: f64) -> usize {
    (value.to_bits() & POINTER_MASK) as usize
}

fn value_of(ptr: *mut crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(ptr_bits(ptr as usize))
}

/// The guards every test here runs under, and a callee that does not move.
struct Fixture {
    _guard: CopyingNurseryTestGuard,
    _triggers: GcTriggerThresholdTestGuard,
    _evacuate: crate::gc::knob_overrides::ForcedEvacuationTestGuard,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            _guard: CopyingNurseryTestGuard::new(0),
            _triggers: GcTriggerThresholdTestGuard::suppress_automatic_triggers(),
            _evacuate: crate::gc::knob_overrides::ForcedEvacuationTestGuard::on(),
        };
        register_runtime_handle_root_scanner_for_tests();
        fixture
    }
}

/// Run `f` with `site` armed and assert the minor ran there.
fn collect_at(site: &'static str, f: impl FnOnce()) {
    crate::gc::arm_collection_point(site);
    let before = crate::gc::copying_minor_cycles();
    f();
    assert!(
        crate::gc::copying_minor_cycles() > before,
        "premise: the armed collection point {site} ran a copying minor"
    );
}

fn assert_moved(now: f64, original: usize, what: &str) {
    assert_ne!(addr(now), original, "premise: {what} moved");
}

/// `Function.prototype.call` with a primitive receiver: binding it is the
/// step that can box, so the arguments cross a collection right after it.
#[test]
fn call_with_a_primitive_receiver_forwards_post_collection_arguments() {
    let _fixture = Fixture::new();
    let target = tenured_closure(crate::fn_info!(record_this_and_two, 2), 0);
    let call = tenured_closure(crate::object::function_prototype_call_thunk_for_test(), 0);
    let scope = RuntimeHandleScope::new();
    let a = scope.root_nanbox_f64(test_string_value(b"first-heap-argument"));
    let b = scope.root_nanbox_f64(test_string_value(b"second-heap-argument"));
    let (a_before, b_before) = (addr(a.get_nanbox_f64()), addr(b.get_nanbox_f64()));
    let receiver = 7.0f64;

    collect_at("explicit_this.coerced", || unsafe {
        let args = [receiver, a.get_nanbox_f64(), b.get_nanbox_f64()];
        crate::closure::native_call_value_this(
            value_of(call),
            crate::closure::JsThis::from_f64(value_of(target)),
            args.as_ptr(),
            args.len(),
        );
    });

    assert_moved(a.get_nanbox_f64(), a_before, "the first argument");
    assert_moved(b.get_nanbox_f64(), b_before, "the second argument");
    assert_eq!(
        seen(),
        vec![
            receiver.to_bits(),
            a.get_nanbox_f64().to_bits(),
            b.get_nanbox_f64().to_bits()
        ],
        "call must forward the arguments' post-collection addresses"
    );
}

/// `call` on an object-literal method that keeps `this` in a capture: the
/// rebind clones it, and a collection after the clone moves the receiver, the
/// clone and the arguments.
#[test]
fn call_on_a_this_capturing_method_forwards_post_collection_values() {
    let _fixture = Fixture::new();
    let target = tenured_closure(
        crate::fn_info!(record_this_and_two, 2),
        crate::closure::CAPTURES_THIS_FLAG | 1,
    );
    let call = tenured_closure(crate::object::function_prototype_call_thunk_for_test(), 0);
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(f64::from_bits(ptr_bits(crate::object::js_object_alloc(
        0, 0,
    ) as usize)));
    let a = scope.root_nanbox_f64(test_string_value(b"captured-this-first"));
    let b = scope.root_nanbox_f64(test_string_value(b"captured-this-second"));
    let receiver_before = addr(receiver.get_nanbox_f64());
    let a_before = addr(a.get_nanbox_f64());

    collect_at("explicit_this.rebound", || unsafe {
        let args = [
            receiver.get_nanbox_f64(),
            a.get_nanbox_f64(),
            b.get_nanbox_f64(),
        ];
        crate::closure::native_call_value_this(
            value_of(call),
            crate::closure::JsThis::from_f64(value_of(target)),
            args.as_ptr(),
            args.len(),
        );
    });

    assert_moved(receiver.get_nanbox_f64(), receiver_before, "the receiver");
    assert_moved(a.get_nanbox_f64(), a_before, "the first argument");
    assert_eq!(
        seen(),
        vec![
            receiver.get_nanbox_f64().to_bits(),
            a.get_nanbox_f64().to_bits(),
            b.get_nanbox_f64().to_bits()
        ],
        "the rebound method must see the post-collection receiver and arguments"
    );
}

/// A bound function over a `this`-capturing method: the bound arguments and
/// the call-time arguments both cross the clone's collection.
#[test]
fn bound_function_forwards_post_collection_values() {
    let _fixture = Fixture::new();
    let target = tenured_closure(
        crate::fn_info!(record_this_and_two, 2),
        crate::closure::CAPTURES_THIS_FLAG | 1,
    );
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(f64::from_bits(ptr_bits(crate::object::js_object_alloc(
        0, 0,
    ) as usize)));
    let a = scope.root_nanbox_f64(test_string_value(b"bound-partial-argument"));
    let b = scope.root_nanbox_f64(test_string_value(b"bound-call-time-argument"));
    let bound = scope.root_nanbox_f64(unsafe {
        let bind_args = [receiver.get_nanbox_f64(), a.get_nanbox_f64()];
        crate::closure::js_function_bind(value_of(target), bind_args.as_ptr(), bind_args.len())
    });
    let b_before = addr(b.get_nanbox_f64());

    collect_at("explicit_this.rebound", || unsafe {
        let args = [b.get_nanbox_f64()];
        crate::closure::native_call_value_this(
            bound.get_nanbox_f64(),
            crate::closure::JsThis::UNDEFINED,
            args.as_ptr(),
            args.len(),
        );
    });

    assert_moved(b.get_nanbox_f64(), b_before, "the call-time argument");
    assert_eq!(
        seen(),
        vec![
            receiver.get_nanbox_f64().to_bits(),
            a.get_nanbox_f64().to_bits(),
            b.get_nanbox_f64().to_bits()
        ],
        "a bound function must forward post-collection bound and call-time arguments"
    );
}

/// `bind` reads its partial arguments after binding the receiver and reading
/// the target's name, both of which can collect.
#[test]
fn bind_roots_its_arguments_across_receiver_binding_and_name_read() {
    for site in ["function_bind.coerced", "function_bind.named"] {
        let _fixture = Fixture::new();
        let target = tenured_closure(crate::fn_info!(record_this_and_two, 2), 0);
        let scope = RuntimeHandleScope::new();
        let a = scope.root_nanbox_f64(test_string_value(b"bind-first-partial"));
        let b = scope.root_nanbox_f64(test_string_value(b"bind-second-partial"));
        let (a_before, b_before) = (addr(a.get_nanbox_f64()), addr(b.get_nanbox_f64()));
        let receiver = 3.0f64;
        let mut bound = 0.0;

        collect_at(site, || unsafe {
            let bind_args = [receiver, a.get_nanbox_f64(), b.get_nanbox_f64()];
            bound = crate::closure::js_function_bind(
                value_of(target),
                bind_args.as_ptr(),
                bind_args.len(),
            );
        });
        let bound = scope.root_nanbox_f64(bound);

        assert_moved(a.get_nanbox_f64(), a_before, "the first partial argument");
        assert_moved(b.get_nanbox_f64(), b_before, "the second partial argument");
        unsafe {
            crate::closure::native_call_value_this(
                bound.get_nanbox_f64(),
                crate::closure::JsThis::UNDEFINED,
                std::ptr::null(),
                0,
            );
        }
        assert_eq!(
            seen(),
            vec![
                receiver.to_bits(),
                a.get_nanbox_f64().to_bits(),
                b.get_nanbox_f64().to_bits()
            ],
            "bind at {site} must capture its partial arguments' post-collection addresses"
        );
    }
}
