//! #9761: `String.prototype.codePointAt` must be answered by the native
//! string-method dispatch, not by the primitive-method FALLBACK.
//!
//! The fallback (`call_primitive_builtin_prototype_method`) resolves
//! `globalThis.String.prototype[<name>]`, clones that closure to rebind `this`,
//! and — because the resolved thunk is not registered strict — runs `ToObject`
//! on the receiver, minting a `String` wrapper with an own index property per
//! UTF-16 code unit. `codePointAt` had a prototype thunk but no dispatch arm,
//! and grapheme-aware text measurement calls it once per character: on the
//! compiled claude-code TUI it was the ONLY name reaching the fallback, at
//! 99,008 calls (and 99,008 wrappers) per 400-character streamed reply.
//!
//! The assertion is the wrapper count, not the return value: a test that only
//! checked the answer would pass with the arm deleted, because the fallback
//! computes the same number — expensively. `BOXED_PRIMITIVE_PAYLOADS` gains one
//! entry per wrapper, so "no new boxed primitives" is exactly "the fallback did
//! not run".

use crate::value::JSValue;

unsafe fn call_string_method(receiver: &str, method: &str, args: &[f64]) -> f64 {
    let s = crate::string::js_string_from_bytes(receiver.as_ptr(), receiver.len() as u32);
    let recv = f64::from_bits(JSValue::string_ptr(s).bits());
    super::js_native_call_method(
        recv,
        method.as_ptr() as *const i8,
        method.len(),
        if args.is_empty() {
            std::ptr::null()
        } else {
            args.as_ptr()
        },
        args.len(),
    )
}

#[test]
fn code_point_at_dispatches_natively_and_boxes_no_receiver() {
    unsafe {
        // Warm anything the first dispatch installs, so the delta below is the
        // method call itself and not one-time globalThis population.
        let _ = call_string_method("ab", "charCodeAt", &[0.0]);
        let before = crate::builtins::test_boxed_primitive_payload_count();

        let cp = call_string_method("a", "codePointAt", &[0.0]);
        assert_eq!(cp, 97.0, "codePointAt(0) of \"a\"");
        let astral = call_string_method("\u{1F600}b", "codePointAt", &[0.0]);
        assert_eq!(astral, 128512.0, "an astral pair is one code point");
        let past_end = call_string_method("a", "codePointAt", &[5.0]);
        assert!(
            JSValue::from_bits(past_end.to_bits()).is_undefined(),
            "out of range is undefined"
        );

        assert_eq!(
            crate::builtins::test_boxed_primitive_payload_count(),
            before,
            "the native arm must not mint a String wrapper; a non-zero delta \
             means the call fell through to the primitive-method fallback"
        );
    }
}

/// Positive control for the assertion above: the counter must be able to move,
/// or "no new boxed primitives" proves nothing. Minting the wrapper the
/// fallback would have minted is the direct, environment-independent form —
/// a second dispatch through a name without a native arm cannot serve as the
/// control here, because the unit-test thread has no populated `globalThis`
/// and the fallback returns before it boxes.
#[test]
fn the_wrapper_counter_moves_when_a_receiver_is_boxed() {
    let before = crate::builtins::test_boxed_primitive_payload_count();
    let s = crate::string::js_string_from_bytes(b"abc".as_ptr(), 3);
    let value = f64::from_bits(JSValue::string_ptr(s).bits());
    let _wrapper = crate::builtins::js_boxed_string_new(value, 1);
    assert!(
        crate::builtins::test_boxed_primitive_payload_count() > before,
        "if this cannot move, the codePointAt assertion above is vacuous"
    );
}

/// #9761 follow-up: the discriminator `call_primitive_closure_value` uses to
/// decide whether a callee is owed a `ToObject` wrapper.
///
/// ECMA-262 §10.3.1 gives a built-in's `[[Call]]` the `thisArg` unchanged, so
/// only a sloppy USER callee gets the wrapper. `builtin_closure_length` is the
/// registry that separates them, and this pins both directions — a
/// `String.prototype` method closure is recognised, a closure the runtime
/// merely allocated is not. Without the negative case the predicate could be
/// "always true" and still pass.
#[test]
fn builtin_prototype_method_closures_are_distinguishable_from_ordinary_ones() {
    let proto = crate::object::global_this::builtin_prototype_value("String");
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *mut crate::object::ObjectHeader;
    assert!(!proto_ptr.is_null(), "String.prototype must exist");

    let key = crate::string::js_string_from_bytes(b"codePointAt".as_ptr(), 11);
    let method = crate::object::js_object_get_field_by_name(proto_ptr, key);
    let method_ptr = crate::value::js_nanbox_get_pointer(f64::from_bits(method.bits())) as usize;
    assert!(method_ptr != 0, "String.prototype.codePointAt must resolve");
    assert!(
        crate::object::builtin_closure_length(method_ptr).is_some(),
        "a String.prototype method closure must read as a built-in"
    );

    extern "C" fn not_a_builtin(_c: *const crate::closure::ClosureHeader) -> f64 {
        0.0
    }
    let ordinary = crate::closure::js_closure_alloc(not_a_builtin as *const u8, 0);
    assert!(
        crate::object::builtin_closure_length(ordinary as usize).is_none(),
        "an ordinary closure must NOT read as a built-in, or every sloppy user \
         callee would silently lose its ToObject wrapper"
    );
}
