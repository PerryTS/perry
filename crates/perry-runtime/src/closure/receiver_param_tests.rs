//! This-as-a-parameter: a JS body is `body(callee, this, a0, ...)`
//! (`perry_abi::JS_BODY_*`), and every runtime route into it passes, as `this`,
//! the receiver its caller supplied — with the JS arguments still in their own
//! positions after it. A plain call supplies `undefined`.
//!
//! Each test calls a probe body with an explicit receiver through one dispatch
//! route (exact arity, padded arity, rest bundling, the padded wide ladder, a
//! hoisted `DirectCallN`), and checks what the probe received.
//! Sabotage: a route passes anything but its `this` argument -> the receiver
//! assertions fail; a route drops the receiver argument (the arguments shift
//! one slot) -> the argument assertions fail.

use super::*;
use std::cell::{Cell, RefCell};

thread_local! {
    static SEEN_THIS: Cell<u64> = const { Cell::new(0) };
    static SEEN_ARGS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

fn seen() -> (u64, Vec<u64>) {
    (
        SEEN_THIS.with(Cell::get),
        SEEN_ARGS.with(|a| a.borrow().clone()),
    )
}

fn record(this: JsThis, args: &[f64]) {
    SEEN_THIS.with(|c| c.set(this.bits()));
    SEEN_ARGS.with(|a| *a.borrow_mut() = args.iter().map(|v| v.to_bits()).collect());
}

extern "C" fn probe3(_c: *const ClosureHeader, this: JsThis, a: f64, b: f64, d: f64) -> f64 {
    record(this, &[a, b, d]);
    a + b
}

extern "C" fn probe_rest(_c: *const ClosureHeader, this: JsThis, a: f64, rest: f64) -> f64 {
    let rest_len = crate::array::js_array_length(
        crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader
    );
    record(this, &[a, rest_len as f64]);
    0.0
}

macro_rules! wide_probe {
    ($($p:ident),+) => {
        extern "C" fn probe_wide(_c: *const ClosureHeader, this: JsThis, $($p: f64),+) -> f64 {
            record(this, &[$($p),+]);
            0.0
        }
    };
}
wide_probe!(
    a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20,
    a21, a22, a23, a24, a25, a26, a27, a28, a29, a30, a31, a32, a33, a34, a35
);

const UNDEF: u64 = crate::value::TAG_UNDEFINED;

fn receiver() -> f64 {
    f64::from_bits(crate::value::JSValue::int32(4242).bits())
}

fn this() -> JsThis {
    JsThis::from_f64(receiver())
}

#[test]
fn an_exact_arity_call_passes_the_receiver_then_the_arguments() {
    let closure = js_closure_alloc(probe3 as *const u8, 0);
    js_register_closure_arity(probe3 as *const u8, 3);
    let r = js_closure_call3(closure, this(), 1.0, 2.0, 3.0);
    assert_eq!(r, 3.0);
    let (this, args) = seen();
    assert_eq!(this, receiver().to_bits(), "the body's `this` parameter");
    assert_eq!(args, vec![1f64.to_bits(), 2f64.to_bits(), 3f64.to_bits()]);
}

#[test]
fn a_padded_call_passes_the_receiver_and_pads_after_it() {
    let closure = js_closure_alloc(probe3 as *const u8, 0);
    js_register_closure_arity(probe3 as *const u8, 3);
    js_closure_call1(closure, this(), 9.0);
    let (this, args) = seen();
    assert_eq!(this, receiver().to_bits());
    assert_eq!(args, vec![9f64.to_bits(), UNDEF, UNDEF]);
}

#[test]
fn a_rest_bundled_call_passes_the_receiver_before_the_fixed_arguments() {
    let closure = js_closure_alloc(probe_rest as *const u8, 0);
    js_register_closure_rest(probe_rest as *const u8, 1);
    js_closure_call4(closure, this(), 5.0, 6.0, 7.0, 8.0);
    let (this, args) = seen();
    assert_eq!(this, receiver().to_bits());
    assert_eq!(
        args,
        vec![5f64.to_bits(), 3f64.to_bits()],
        "fixed arg, then rest length"
    );
}

#[test]
fn a_wide_call_passes_the_receiver_and_every_argument_slot() {
    let closure = js_closure_alloc(probe_wide as *const u8, 0);
    js_register_closure_arity(probe_wide as *const u8, 36);
    let args: Vec<f64> = (0..36).map(f64::from).collect();
    let r =
        unsafe { js_closure_call_array(closure as i64, this(), args.as_ptr(), args.len() as i64) };
    assert_eq!(r, 0.0);
    let (this, seen_args) = seen();
    assert_eq!(this, receiver().to_bits());
    assert_eq!(
        seen_args,
        args.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
}

#[test]
fn a_hoisted_direct_call_passes_the_receiver() {
    let closure = js_closure_alloc(probe3 as *const u8, 0);
    js_register_closure_arity(probe3 as *const u8, 3);
    let site = DirectCall3::resolve(closure);
    assert!(site.is_direct(), "the probe resolves to a direct call");
    site.call(closure, this(), 1.0, 2.0, 3.0);
    let (this, args) = seen();
    assert_eq!(this, receiver().to_bits());
    assert_eq!(args, vec![1f64.to_bits(), 2f64.to_bits(), 3f64.to_bits()]);
}

#[test]
fn a_plain_call_passes_undefined_even_inside_a_receiver_call() {
    let closure = js_closure_alloc(probe3 as *const u8, 0);
    js_register_closure_arity(probe3 as *const u8, 3);
    js_closure_call3(closure, this(), 1.0, 2.0, 3.0);
    js_closure_call3(
        closure,
        crate::closure::plain_call_receiver(),
        4.0,
        5.0,
        6.0,
    );
    let (this, args) = seen();
    assert_eq!(
        this, UNDEF,
        "a plain call must not inherit an earlier receiver"
    );
    assert_eq!(args, vec![4f64.to_bits(), 5f64.to_bits(), 6f64.to_bits()]);
}
