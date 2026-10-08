//! `Function.prototype.call` and `.bind` take their arguments in place
//! (`FN_REST_NATIVE_ARGS`): forwarding a call through them builds no rest
//! array. The uncurried `call.bind(WeakMap.prototype.get)` idiom of
//! call-bound / side-channel / qs runs it for every get/set.

use std::cell::RefCell;

crate::perry_thread_local! {
    static SEEN: RefCell<Vec<u64>> = RefCell::new(Vec::new());
}

extern "C" fn record_this_and_two(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    a: f64,
    b: f64,
) -> f64 {
    SEEN.with(|seen| *seen.borrow_mut() = vec![this.bits(), a.to_bits(), b.to_bits()]);
    a + b
}

fn take_seen() -> Vec<u64> {
    SEEN.with(|seen| std::mem::take(&mut *seen.borrow_mut()))
}

struct Scope {
    _suppress: crate::gc::GcSuppressScope,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Scope {
    fn new() -> Self {
        Self {
            _lock: crate::gc::global_side_table_test_lock(),
            _suppress: crate::gc::GcSuppressScope::new(),
        }
    }
}

fn closure_value(info: *const crate::closure::JsFunctionInfo) -> f64 {
    crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(info, 0) as i64)
}

fn call_info() -> *const crate::closure::JsFunctionInfo {
    crate::fn_info!(native_args super::function_prototype_call_thunk, 1; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR))
}

fn bind_info() -> *const crate::closure::JsFunctionInfo {
    crate::fn_info!(native_args super::function_prototype_bind_thunk, 1; with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR))
}

fn arena_bytes_during<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let before = crate::arena::arena_in_use_bytes();
    let result = f();
    (result, crate::arena::arena_in_use_bytes() - before)
}

#[test]
fn native_args_bodies_keep_their_spec_arity() {
    let info = unsafe { &*call_info() };
    assert!(matches!(
        crate::closure::info_rest(info),
        Some((1, crate::closure::RestDispatchKind::NativeArgs))
    ));
    assert_eq!(crate::closure::info_arity(info), Some(1));
}

#[test]
fn uncurried_call_forwards_without_a_rest_array() {
    let _scope = Scope::new();
    unsafe {
        let call = closure_value(call_info());
        let target = closure_value(crate::fn_info!(record_this_and_two, 2; with_declared(2)));
        // `const uncurried = Function.prototype.call.bind(target)`.
        let uncurried = crate::closure::js_function_bind(call, &target, 1);
        let receiver = crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
        let args = [receiver, 2.0, 5.0];
        let bound_ptr = crate::value::js_nanbox_get_pointer(uncurried) as *const _;
        // Warm every lazily built per-body fact before measuring.
        crate::closure::native_call_value_this(
            uncurried,
            crate::closure::JsThis::UNDEFINED,
            args.as_ptr(),
            args.len(),
        );
        take_seen();

        let (results, bytes) = arena_bytes_during(|| {
            let mut results = Vec::new();
            for _ in 0..64 {
                results.push(crate::closure::native_call_value_this(
                    uncurried,
                    crate::closure::JsThis::UNDEFINED,
                    args.as_ptr(),
                    args.len(),
                ));
                results.push(crate::closure::js_closure_call3(
                    bound_ptr,
                    crate::closure::JsThis::UNDEFINED,
                    receiver,
                    2.0,
                    5.0,
                ));
            }
            results
        });
        assert!(
            results.iter().all(|r| *r == 7.0),
            "the target ran with both args"
        );
        assert_eq!(
            take_seen(),
            vec![receiver.to_bits(), 2.0f64.to_bits(), 5.0f64.to_bits()],
            "the explicit receiver and the args after it reach the target"
        );
        assert_eq!(
            bytes, 0,
            "forwarding through call must not build a rest array"
        );
    }
}

#[test]
fn call_and_bind_with_missing_arguments() {
    let _scope = Scope::new();
    unsafe {
        let call = closure_value(call_info());
        let bind = closure_value(bind_info());
        let target = closure_value(crate::fn_info!(record_this_and_two, 2; with_declared(2)));
        let undef = crate::value::TAG_UNDEFINED;

        // `target.call()`: receiver and both args are undefined.
        crate::closure::native_call_value_this(
            call,
            crate::closure::JsThis::from_f64(target),
            std::ptr::null(),
            0,
        );
        assert_eq!(take_seen(), vec![undef, undef, undef]);

        // `target.bind(thisArg, 3)(4)`.
        let this_arg = 11.0f64;
        let bind_args = [this_arg, 3.0];
        let bound = crate::closure::native_call_value_this(
            bind,
            crate::closure::JsThis::from_f64(target),
            bind_args.as_ptr(),
            bind_args.len(),
        );
        let result = crate::closure::native_call_value_this(
            bound,
            crate::closure::JsThis::UNDEFINED,
            &4.0,
            1,
        );
        assert_eq!(result, 7.0);
        assert_eq!(
            take_seen(),
            vec![this_arg.to_bits(), 3.0f64.to_bits(), 4.0f64.to_bits()]
        );

        // `target.bind()()`: an undefined receiver, no partial args.
        let bare = crate::closure::native_call_value_this(
            bind,
            crate::closure::JsThis::from_f64(target),
            std::ptr::null(),
            0,
        );
        crate::closure::native_call_value_this(
            bare,
            crate::closure::JsThis::UNDEFINED,
            [1.0, 2.0].as_ptr(),
            2,
        );
        assert_eq!(take_seen(), vec![undef, 1.0f64.to_bits(), 2.0f64.to_bits()]);
    }
}
