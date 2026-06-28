//! Promise combinators — `Promise.all`, `Promise.race`, `Promise.any`,
//! `Promise.allSettled` — plus the executor entry, thenable
//! assimilation, the scheduled-resolve queue, and `is_promise` probes.

use super::*;
use std::os::raw::c_int;

use super::assimilate::{
    assimilate_via_then_property, enqueue_thenable_job, get_then_action,
    promise_resolve_assimilating, thenable_job_reject_fn, thenable_job_resolve_fn,
};

#[derive(Clone, Copy)]
pub(super) struct PromiseAllState {
    pub result_promise: *mut Promise,
    pub results_arr: *mut crate::array::ArrayHeader,
    pub state_arr: *mut crate::array::ArrayHeader,
    pub index: u32,
}

thread_local! {
    pub(super) static PROMISE_ALL_STATES: RefCell<Vec<(usize, PromiseAllState)>> =
        const { RefCell::new(Vec::new()) };
}

/// Drain ALL `PromiseAllState` entries associated with `promise`.
///
/// A pending promise can be reused as an input to multiple
/// `Promise.all([...])` calls — e.g.:
///
/// ```js
/// const p = somePending();
/// Promise.all([p, x]);
/// Promise.all([p, y]);
/// ```
///
/// Each `Promise.all` call registers its own `PromiseAllState` keyed by
/// `p as usize`. When `p` settles we must complete every registered
/// state, not just the first one.
#[inline]
pub(super) fn promise_all_take_all_handlers(promise: *mut Promise) -> Vec<PromiseAllState> {
    if promise.is_null() {
        return Vec::new();
    }
    PROMISE_ALL_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let key = promise as usize;
        let mut drained = Vec::new();
        let mut i = 0;
        while i < states.len() {
            if states[i].0 == key {
                drained.push(states.swap_remove(i).1);
            } else {
                i += 1;
            }
        }
        drained
    })
}

#[inline]
pub(super) fn promise_all_settle(state: PromiseAllState, value: f64, is_fulfilled: bool) {
    if is_fulfilled {
        promise_all_fulfill_direct(state, value);
    } else {
        promise_all_reject_direct(state.result_promise, state.state_arr, value);
    }
}

pub(super) fn scan_promise_all_states_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROMISE_ALL_STATES.with(|states| {
        let mut states = states.borrow_mut();
        for (key, state) in states.iter_mut() {
            visitor.visit_metadata_usize_slot(key);
            visitor.visit_raw_mut_ptr_slot(&mut state.result_promise);
            visitor.visit_raw_mut_ptr_slot(&mut state.results_arr);
            visitor.visit_raw_mut_ptr_slot(&mut state.state_arr);
        }
    });
}

/// Create a rejected promise with the given reason
#[no_mangle]
pub extern "C" fn js_promise_rejected(reason: f64) -> *mut Promise {
    let promise = js_promise_new();
    js_promise_reject(promise, reason);
    promise
}

fn string_header_to_string(ptr: *const crate::string::StringHeader) -> String {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        if len > 1 << 30 {
            return String::new();
        }
        let bytes_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bytes = std::slice::from_raw_parts(bytes_ptr, len);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn promise_try_type_error_value(callback: f64) -> f64 {
    let type_name = string_header_to_string(crate::builtins::js_value_typeof(callback));
    let rendered = string_header_to_string(crate::value::js_jsvalue_to_string(callback));
    let message = match (type_name.as_str(), rendered.as_str()) {
        ("", "") => "value is not a function".to_string(),
        (_, "") => format!("{type_name} is not a function"),
        ("undefined", _) => "undefined is not a function".to_string(),
        _ => format!("{type_name} {rendered} is not a function"),
    };
    let message_ptr = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_typeerror_new(message_ptr);
    let bits = crate::value::JSValue::pointer(error as *const u8).bits();
    f64::from_bits(bits)
}

fn promise_try_closure_ptr(callback: f64) -> Option<*const crate::closure::ClosureHeader> {
    let ptr = crate::value::js_nanbox_get_pointer(callback) as usize;
    if crate::value::addr_class::is_handle_band(ptr) {
        return None;
    }
    crate::closure::is_closure_ptr(ptr).then_some(ptr as *const crate::closure::ClosureHeader)
}

fn promise_try_call(callback: f64, args_ptr: *const f64, args_len: usize) -> Result<f64, f64> {
    let Some(closure) = promise_try_closure_ptr(callback) else {
        return Err(promise_try_type_error_value(callback));
    };

    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    let result = if jumped == 0 {
        let value = unsafe {
            crate::closure::js_closure_call_array(closure as i64, args_ptr, args_len as i64)
        };
        Ok(value)
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(exc)
    };
    crate::exception::js_try_end();
    result
}

/// `Promise.try(fn, ...args)`: call `fn` with forwarded args and normalize the
/// result or synchronous throw into a Promise.
#[no_mangle]
pub extern "C" fn js_promise_try(
    callback: f64,
    args: *const crate::array::ArrayHeader,
) -> *mut Promise {
    let (args_ptr, args_len) = if args.is_null() {
        (std::ptr::null(), 0)
    } else {
        let len = unsafe { (*args).length as usize };
        let data = unsafe {
            (args as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64
        };
        (data, len)
    };

    match promise_try_call(callback, args_ptr, args_len) {
        Ok(value) => js_promise_resolved(value),
        Err(reason) => js_promise_rejected(reason),
    }
}

/// Check if a value is a promise (by checking if it's a valid pointer)
/// This is a simplified check - in reality we'd need type tags
#[no_mangle]
pub extern "C" fn js_is_promise(ptr: *mut Promise) -> i32 {
    if ptr.is_null() {
        return 0;
    }
    // Basic sanity check - could be more sophisticated
    1
}

/// Safe `await`-side check: given a NaN-boxed JSValue, return 1 if it
/// points at a real Promise allocation and 0 otherwise. Used by the
/// LLVM backend's `Expr::Await` lowering so that `await <non-promise>`
/// doesn't dereference a garbage pointer as if it were a `Promise`.
///
/// Inspects the NaN-box tag and, when the value is a pointer, walks
/// back to the `GcHeader` to read the `obj_type`. Any non-POINTER_TAG
/// bits (primitives, strings, bigints, null, undefined) return 0.
#[no_mangle]
pub extern "C" fn js_value_is_promise(value: f64) -> i32 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
    // #854: POINTER_MASK part of the NaN-boxing tag contract — referenced
    // by sibling helpers and kept here so the constants stay co-located.
    #[allow(dead_code)]
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    let bits = value.to_bits();
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG {
        return 0;
    }
    let ptr_usize = (bits & crate::value::POINTER_MASK) as usize;
    // Pointer-tagged native handles (Fetch/Headers/Timers/etc.) also carry
    // small payloads. They are not GC allocations and must not be probed as
    // Promise headers before the handle dispatch tables see them.
    if crate::value::addr_class::is_handle_band(ptr_usize) {
        return 0;
    }
    // Off-GC-heap header-less allocations (small typed arrays / buffers) are
    // never Promises and must not be header-probed (#5226).
    if crate::typedarray::is_offheap_sidetable_alloc(ptr_usize) {
        return 0;
    }
    unsafe {
        let gc_header =
            (ptr_usize as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let ot = (*gc_header).obj_type;
        if ot == crate::gc::GC_TYPE_PROMISE {
            1
        } else {
            0
        }
    }
}

/// Issue #2822: compute the Node-compatible prefix for the
/// "<x> is not iterable (cannot read property Symbol(Symbol.iterator))"
/// TypeError message for a non-iterable combinator argument.
///
/// Node's `%TypeError%` text varies by `typeof`:
///   number 1 / number -3.5 / boolean true → "<type> <value>"
///   object null                            → "object null"
///   undefined / object / symbol / function / bigint → "<type>"
fn not_iterable_prefix(value: f64) -> String {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        return "undefined".to_string();
    }
    if jsval.is_null() {
        return "object null".to_string();
    }
    if jsval.is_number() {
        let s = crate::value::js_jsvalue_to_string(value);
        let num_str = crate::string::string_as_str(s);
        return format!("number {}", num_str);
    }
    if jsval.is_bool() {
        let b = value.to_bits() == crate::value::TAG_TRUE;
        return format!("boolean {}", b);
    }
    if jsval.is_bigint() {
        return "bigint".to_string();
    }
    if jsval.is_any_string() {
        // Strings ARE iterable; this branch should never be reached for
        // strings, but keep it defensive.
        return "string".to_string();
    }
    if jsval.is_pointer() {
        let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::symbol::is_registered_symbol(raw) {
            return "symbol".to_string();
        }
        if crate::closure::is_closure_ptr(raw) {
            return "function".to_string();
        }
        return "object".to_string();
    }
    "object".to_string()
}

pub(super) fn combinator_catch_js<F: FnOnce() -> f64>(f: F) -> Result<f64, f64> {
    let env = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut c_int) };
    if jumped == 0 {
        let result = f();
        crate::exception::js_try_end();
        Ok(result)
    } else {
        crate::exception::js_try_end();
        let err = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(err)
    }
}

/// Build the `TypeError: <prefix> is not iterable (cannot read property
/// Symbol(Symbol.iterator))` value — Node's exact message for a non-iterable
/// Promise-combinator argument (issue #2822).
pub(super) fn not_iterable_error_value(value: f64) -> f64 {
    let msg = format!(
        "{} is not iterable (cannot read property Symbol(Symbol.iterator))",
        not_iterable_prefix(value)
    );
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    f64::from_bits(err_value)
}

/// Issue #2822: decide whether a boxed pointer value is iterable, and if so
/// return its elements as a flat array. Returns `Ok(arr)` for iterables
/// (arrays, Set, Map, strings, generators / objects with `[Symbol.iterator]`,
/// iterator objects with a `.next` field) and `Err(())` for everything else.
///
/// Reuses `js_array_clone`'s established iterable-collection path (the same
/// engine that backs spread / `Array.from`), but gates it behind an explicit
/// iterability probe so non-iterable primitives and plain objects reject with
/// a `TypeError` instead of silently coercing to `[]`.
pub(crate) fn combinator_iterable_to_array(
    value: f64,
) -> Result<*mut crate::array::ArrayHeader, f64> {
    use crate::value::JSValue;

    // Arrays and strings are always iterable.
    if crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE {
        return Ok(crate::array::js_array_clone(
            crate::value::js_nanbox_get_pointer(value) as *const crate::array::ArrayHeader,
        ));
    }
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        return Ok(crate::array::js_array_clone(
            crate::value::js_nanbox_get_pointer(value) as *const crate::array::ArrayHeader,
        ));
    }
    if !jsval.is_pointer() {
        return Err(not_iterable_error_value(value));
    }

    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if crate::value::addr_class::is_handle_band(raw) {
        return Err(not_iterable_error_value(value));
    }

    // Side-table iterables.
    if crate::set::is_registered_set(raw) || crate::map::is_registered_map(raw) {
        return Ok(crate::array::js_array_clone(
            raw as *const crate::array::ArrayHeader,
        ));
    }
    if crate::buffer::is_registered_buffer(raw) {
        return Ok(crate::array::js_array_clone(
            raw as *const crate::array::ArrayHeader,
        ));
    }

    // Symbols / closures are not iterable.
    if crate::symbol::is_registered_symbol(raw) || crate::closure::is_closure_ptr(raw) {
        return Err(not_iterable_error_value(value));
    }

    // GC_TYPE_OBJECT: iterable only if it exposes `[Symbol.iterator]` or a
    // `.next` closure field (a bare iterator object).
    let obj_type = unsafe {
        if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return Err(not_iterable_error_value(value));
        }
        let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*hdr).obj_type
    };
    if obj_type == crate::gc::GC_TYPE_OBJECT {
        let has_iterator = {
            let iter_sym = crate::symbol::well_known_symbol("iterator");
            if iter_sym.is_null() {
                false
            } else {
                let sym_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());
                let iter_fn =
                    unsafe { crate::symbol::js_object_get_symbol_property(value, sym_f64) };
                iter_fn.to_bits() != crate::value::TAG_UNDEFINED
            }
        };
        let has_next_field = {
            let next_key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
            let next_val = crate::object::js_object_get_field_by_name(
                raw as *const crate::object::ObjectHeader,
                next_key,
            );
            let next_ptr = crate::value::js_nanbox_get_pointer(f64::from_bits(next_val.bits()));
            !next_val.is_undefined() && crate::closure::is_closure_ptr(next_ptr as usize)
        };
        if has_iterator || has_next_field {
            return Ok(crate::array::js_array_clone(
                raw as *const crate::array::ArrayHeader,
            ));
        }
    }

    Err(not_iterable_error_value(value))
}

pub(super) fn combinator_iterable_to_array_caught(
    value: f64,
) -> Result<*mut crate::array::ArrayHeader, f64> {
    let mut out = std::ptr::null_mut();
    let result = combinator_catch_js(|| match combinator_iterable_to_array(value) {
        Ok(arr) => {
            out = arr;
            0.0
        }
        Err(reason) => reason,
    });
    match result {
        Ok(_) if !out.is_null() => Ok(out),
        Ok(reason) | Err(reason) => Err(reason),
    }
}

/// Unwrap a spec-combinator result (NaN-boxed native Promise pointer, since the
/// codegen direct-call path always uses the intrinsic `Promise` as `this`) back
/// to a `*mut Promise` for the codegen ABI.
#[inline]
fn spec_result_to_promise(result: f64) -> *mut Promise {
    crate::value::js_nanbox_get_pointer(result) as *mut Promise
}

/// Iterable-accepting entry for `Promise.all` (issue #2822 / #4521). Codegen's
/// direct-call path (`Promise.all([...])`) lands here with `this` = intrinsic
/// `Promise`; supply that constructor explicitly and delegate to the
/// spec-compliant combinator.
#[no_mangle]
pub extern "C" fn js_promise_all_iterable(value: f64) -> *mut Promise {
    let c = super::spec_combinators::default_promise_ctor();
    spec_result_to_promise(super::spec_combinators::js_promise_all_spec(c, value))
}

/// Iterable-accepting entry for `Promise.race` (issue #2822 / #4521).
#[no_mangle]
pub extern "C" fn js_promise_race_iterable(value: f64) -> *mut Promise {
    let c = super::spec_combinators::default_promise_ctor();
    spec_result_to_promise(super::spec_combinators::js_promise_race_spec(c, value))
}

/// Iterable-accepting entry for `Promise.allSettled` (issue #2822 / #4521).
#[no_mangle]
pub extern "C" fn js_promise_all_settled_iterable(value: f64) -> *mut Promise {
    let c = super::spec_combinators::default_promise_ctor();
    spec_result_to_promise(super::spec_combinators::js_promise_all_settled_spec(
        c, value,
    ))
}

/// Iterable-accepting entry for `Promise.any` (issue #2822 / #4521).
#[no_mangle]
pub extern "C" fn js_promise_any_iterable(value: f64) -> *mut Promise {
    let c = super::spec_combinators::default_promise_ctor();
    spec_result_to_promise(super::spec_combinators::js_promise_any_spec(c, value))
}

/// #2822/#3320: keepalive anchors so the whole-program LLVM (auto-optimize)
/// build does not dead-strip these codegen-only `#[no_mangle]` entry points.
#[used]
static KEEP_PROMISE_ALL_ITERABLE: extern "C" fn(f64) -> *mut Promise = js_promise_all_iterable;
#[used]
static KEEP_PROMISE_RACE_ITERABLE: extern "C" fn(f64) -> *mut Promise = js_promise_race_iterable;
#[used]
static KEEP_PROMISE_ALL_SETTLED_ITERABLE: extern "C" fn(f64) -> *mut Promise =
    js_promise_all_settled_iterable;
#[used]
static KEEP_PROMISE_ANY_ITERABLE: extern "C" fn(f64) -> *mut Promise = js_promise_any_iterable;

// Queue for scheduled promise resolutions
thread_local! {
    pub(in crate::promise) static SCHEDULED_RESOLVES: RefCell<Vec<(*mut Promise, f64)>> = const { RefCell::new(Vec::new()) };
}

/// Schedule a promise to be resolved with a value when microtasks run
/// This simulates an async operation completing
#[no_mangle]
pub extern "C" fn js_promise_schedule_resolve(promise: *mut Promise, value: f64) {
    SCHEDULED_RESOLVES.with(|q| {
        q.borrow_mut().push((promise, value));
    });
}

/// Process scheduled resolutions (called by js_promise_run_microtasks)
pub(super) fn process_scheduled_resolves() -> i32 {
    let mut count = 0;
    loop {
        let item = SCHEDULED_RESOLVES.with(|q| q.borrow_mut().pop());
        match item {
            Some((promise, value)) => {
                js_promise_resolve(promise, value);
                count += 1;
            }
            None => break,
        }
    }
    count
}

/// Create a new Promise with an executor callback.
/// The executor receives (resolve, reject) as arguments.
/// resolve and reject are closures that call js_promise_resolve/js_promise_reject.
///
/// Arguments:
/// - executor: A closure that takes 2 arguments (resolve_fn, reject_fn)
#[no_mangle]
pub extern "C" fn js_promise_new_with_executor(
    executor: *const crate::closure::ClosureHeader,
) -> *mut Promise {
    use crate::closure::js_closure_call2;

    let promise = js_promise_new();

    // Create the resolve/reject pair sharing a [[AlreadyResolved]] guard, so
    // calling one disables the other (27.2.1.3 CreateResolvingFunctions). The
    // resolve fn also assimilates thenables/promises and rejects self-resolution.
    let (resolve_closure, reject_closure) = make_resolving_functions(promise);

    // Call the executor with (resolve_closure, reject_closure) as proper
    // NaN-boxed POINTER_TAG closure values — so user code that *reflects* on
    // them (`resolve.length` === 1, `resolve.name` === "", `new resolve()`
    // throws, `Object.getOwnPropertyNames(resolve)`) sees a real function
    // object, not a bare number. The call path already accepts POINTER_TAG
    // closures (this mirrors `NewPromiseCapability`'s fast path, which has
    // always boxed them). test262 `resolve-function-*` / `reject-function-*`.
    let resolve_f64: f64 = crate::value::js_nanbox_pointer(resolve_closure as i64);
    let reject_f64: f64 = crate::value::js_nanbox_pointer(reject_closure as i64);
    // 27.2.3.1: a non-callable executor throws a TypeError SYNCHRONOUSLY at
    // step 2 (before/instead of being run) — that throw must propagate out of
    // `new Promise(...)`, not be converted into a rejection. Only a genuinely
    // callable executor's abrupt completion (step 10) is caught and rejected.
    if crate::closure::is_closure_ptr(executor as usize) {
        // Callable executor: catch a throw from its body and reject the promise
        // with the thrown value via the resolving `reject` function (so the
        // shared [[AlreadyResolved]] guard makes a throw AFTER a resolve/reject a
        // no-op). test262 reject-via-abrupt / exception-after-resolve-in-executor.
        if let Err(reason) =
            combinator_catch_js(|| js_closure_call2(executor, resolve_f64, reject_f64))
        {
            crate::closure::js_closure_call1(reject_closure, reason);
        }
    } else {
        // Non-callable: preserve the prior synchronous TypeError (uncaught).
        js_closure_call2(executor, resolve_f64, reject_f64);
    }

    promise
}

/// A resolving function's shared `[[AlreadyResolved]]` record. Per ECMA-262
/// 27.2.1.3 CreateResolvingFunctions, the resolve and reject functions handed to
/// a Promise executor share ONE boolean: once either fires, the other is a
/// no-op — *even while the promise itself is still pending* (e.g. `resolve` was
/// called with an unsettled thenable, so the promise stays pending while the
/// thenable job runs, but a later `reject(x)` must be ignored). The `state !=
/// Pending` guard inside `js_promise_resolve`/`js_promise_reject` is NOT enough
/// to model this; we need the explicit flag.
///
/// Stored as a 1-element array (slot 0: 0.0 = not resolved, 1.0 = resolved) so
/// the resolve and reject closures can both capture the same heap cell. A NULL
/// capture means "no shared guard" (legacy single-shot callers that rely solely
/// on the promise-state check).
fn alloc_already_resolved_guard() -> *mut crate::array::ArrayHeader {
    use crate::array::{js_array_alloc, js_array_set_f64};
    let guard = js_array_alloc(1);
    unsafe {
        (*guard).length = 1;
    }
    js_array_set_f64(guard, 0, 0.0);
    guard
}

/// Take the shared `[[AlreadyResolved]]` flag: returns true if this resolving
/// function may proceed (and marks it resolved), false if already consumed. A
/// NULL guard always proceeds.
#[inline]
fn take_already_resolved(guard: *mut crate::array::ArrayHeader) -> bool {
    use crate::array::{js_array_get_f64, js_array_set_f64};
    if guard.is_null() {
        return true;
    }
    if js_array_get_f64(guard, 0) != 0.0 {
        return false;
    }
    js_array_set_f64(guard, 0, 1.0);
    true
}

/// Register the dispatch arity (= 1) of the native resolving / thenable-job
/// functions, once per thread. Each is an `extern "C" fn(closure, value)`, so a
/// JS call that passes FEWER arguments — a thenable whose `then` does
/// `resolve()` with no args (27.2.1.3.2 step 9) — must pad the missing `value`
/// to `undefined`. Without a registered arity, `js_closure_call0` falls to the
/// zero-arg direct-call arm and the function reads a GARBAGE second register
/// (observed as the denormal `5e-324`, i.e. bits = 1), corrupting the
/// resolution value. (test262 exception-after-resolve-in-{executor,thenable-job}.)
pub(super) fn ensure_native_resolving_arity_registered() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    DONE.with(|d| {
        if d.get() {
            return;
        }
        d.set(true);
        for f in [
            promise_resolve_fn as *const u8,
            promise_reject_fn as *const u8,
            thenable_job_resolve_fn as *const u8,
            thenable_job_reject_fn as *const u8,
        ] {
            crate::closure::js_register_closure_arity(f, 1);
        }
    });
}

/// Wire a resolve/reject closure pair to a promise with a shared
/// `[[AlreadyResolved]]` guard. Returns `(resolve_closure, reject_closure)`.
pub(super) fn make_resolving_functions(
    promise: *mut Promise,
) -> (
    *mut crate::closure::ClosureHeader,
    *mut crate::closure::ClosureHeader,
) {
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};
    ensure_native_resolving_arity_registered();
    let guard = alloc_already_resolved_guard();
    let resolve = js_closure_alloc(promise_resolve_fn as *const u8, 2);
    js_closure_set_capture_ptr(resolve, 0, promise as i64);
    js_closure_set_capture_ptr(resolve, 1, guard as i64);
    let reject = js_closure_alloc(promise_reject_fn as *const u8, 2);
    js_closure_set_capture_ptr(reject, 0, promise as i64);
    js_closure_set_capture_ptr(reject, 1, guard as i64);
    // Spec 27.2.1.3: the resolving functions are anonymous built-in functions
    // with own `length` = 1, `name` = "" (both non-writable, non-enumerable,
    // configurable), and NO `[[Construct]]` (`new resolve()` throws). test262
    // `resolve-function-*` / `reject-function-*` assert all four.
    for f in [resolve, reject] {
        crate::object::set_builtin_closure_length(f as usize, 1);
        crate::object::set_bound_native_closure_name(f, "");
        crate::object::set_builtin_closure_non_constructable(f as usize);
    }
    (resolve, reject)
}

/// Internal resolve function for Promise executor callbacks.
/// Called when user calls resolve(value) inside the executor.
///
/// Implements ECMA-262 27.2.1.3.2 Promise Resolve Functions:
///   * honour the shared `[[AlreadyResolved]]` flag (capture slot 1);
///   * if `resolution` is the promise itself, reject with a TypeError;
///   * if `resolution` is a thenable/promise, assimilate it (enqueue a job)
///     rather than fulfilling with the thenable as a plain value.
pub(super) extern "C" fn promise_resolve_fn(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;

    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let promise_ptr = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let guard = js_closure_get_capture_ptr(closure, 1) as *mut crate::array::ArrayHeader;
    if !take_already_resolved(guard) {
        return undef;
    }

    // Self-resolution: `resolve(thePromise)` rejects with a TypeError (27.2.1.3.2
    // step 6). Detect by comparing the resolution value's pointer to the promise.
    if !promise_ptr.is_null() {
        let bits = value.to_bits();
        if (bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG {
            let ptr = (bits & crate::value::POINTER_MASK) as usize;
            if ptr == promise_ptr as usize {
                let msg = b"Chaining cycle detected for promise #<Promise>";
                let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
                let err_ptr = crate::error::js_typeerror_new(s);
                let err =
                    f64::from_bits(crate::value::JSValue::pointer(err_ptr as *const u8).bits());
                js_promise_reject(promise_ptr, err);
                return undef;
            }
        }
    }

    promise_resolve_assimilating(promise_ptr, value);
    undef // resolve returns undefined
}

/// Internal reject function for Promise executor callbacks.
/// Called when user calls reject(reason) inside the executor.
pub(super) extern "C" fn promise_reject_fn(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;

    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let promise_ptr = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let guard = js_closure_get_capture_ptr(closure, 1) as *mut crate::array::ArrayHeader;
    if !take_already_resolved(guard) {
        return undef;
    }
    js_promise_reject(promise_ptr, reason);
    undef // reject returns undefined
}

#[inline]
pub(super) fn callable_closure_value(value: f64) -> Option<*const crate::closure::ClosureHeader> {
    let bits = value.to_bits();
    let tag = bits & crate::value::TAG_MASK;
    let raw = if tag == crate::value::POINTER_TAG || tag == crate::value::STRING_TAG {
        (bits & crate::value::POINTER_MASK) as usize
    } else {
        bits as usize
    };
    if raw >= 0x10000
        && raw % std::mem::align_of::<u32>() == 0
        && crate::closure::is_closure_ptr(raw)
    {
        Some(raw as *const crate::closure::ClosureHeader)
    } else {
        None
    }
}

fn promise_resolve_for_combinator(value: f64) -> Result<*mut Promise, f64> {
    let value = adapt_foreign_promise_value(value);
    if js_value_is_promise(value) != 0 {
        let promise = crate::value::js_nanbox_get_pointer(value) as *mut Promise;
        if !promise.is_null() {
            return Ok(promise);
        }
    }

    match get_then_action(value) {
        Ok(Some(then_action)) => {
            let promise = js_promise_new();
            enqueue_thenable_job(promise, value, then_action);
            return Ok(promise);
        }
        Ok(None) => {}
        Err(reason) => {
            let promise = js_promise_new();
            js_promise_reject(promise, reason);
            return Ok(promise);
        }
    }

    let promise = js_promise_new();
    js_promise_resolve(promise, value);
    Ok(promise)
}

/// Promise.all - takes an array of promises and returns a promise that resolves
/// with an array of all resolved values, or rejects if any promise rejects.
///
/// Arguments:
/// - promises_arr: pointer to an ArrayHeader containing promise pointers (as NaN-boxed f64)
///
/// Returns: a new Promise that resolves with an array of results
#[no_mangle]
pub extern "C" fn js_promise_all(promises_arr: *const crate::array::ArrayHeader) -> *mut Promise {
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        let empty_arr = js_array_alloc(0);
        unsafe {
            (*empty_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        promise_resolve_assimilating(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);

    if count == 0 {
        let empty_arr = js_array_alloc(0);
        unsafe {
            (*empty_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        promise_resolve_assimilating(result_promise, arr_f64);
        return result_promise;
    }

    let results_arr = js_array_alloc(count);
    unsafe {
        (*results_arr).length = count;
    }

    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(results_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    let state_arr = js_array_alloc(2);
    unsafe {
        (*state_arr).length = 2;
    }
    js_array_set_f64(state_arr, 0, count as f64);
    js_array_set_f64(state_arr, 1, 0.0);

    for i in 0..count {
        let promise_ptr = match promise_resolve_for_combinator(js_array_get_f64(promises_arr, i)) {
            Ok(promise) => promise,
            Err(reason) => {
                promise_all_reject_direct(result_promise, state_arr, reason);
                break;
            }
        };
        let state = PromiseAllState {
            result_promise,
            results_arr,
            state_arr,
            index: i,
        };

        unsafe {
            match (*promise_ptr).state {
                PromiseState::Fulfilled => {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::PromiseAll(
                            state,
                            (*promise_ptr).value,
                            true,
                            context_for_promise(promise_ptr),
                        ));
                    });
                }
                PromiseState::Rejected => {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::PromiseAll(
                            state,
                            (*promise_ptr).reason,
                            false,
                            context_for_promise(promise_ptr),
                        ));
                    });
                }
                PromiseState::Pending => {
                    PROMISE_ALL_STATES.with(|states| {
                        states.borrow_mut().push((promise_ptr as usize, state));
                    });
                    set_promise_callback_context(promise_ptr);
                }
            }
        }
    }

    let remaining = js_array_get_f64(state_arr, 0);
    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        promise_resolve_assimilating(result_promise, arr_f64);
    }

    result_promise
}

#[inline]
fn promise_all_fulfill_direct(state: PromiseAllState, value: f64) {
    use crate::array::{js_array_get_f64, js_array_set_f64};

    if state.result_promise.is_null() || state.results_arr.is_null() || state.state_arr.is_null() {
        return;
    }

    let rejected = js_array_get_f64(state.state_arr, 1);
    if rejected != 0.0 {
        return;
    }

    js_array_set_f64(state.results_arr, state.index, value);
    let remaining = js_array_get_f64(state.state_arr, 0) - 1.0;
    js_array_set_f64(state.state_arr, 0, remaining);

    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(state.results_arr as i64);
        promise_resolve_assimilating(state.result_promise, arr_f64);
    }
}

#[inline]
fn promise_all_reject_direct(
    result_promise: *mut Promise,
    state_arr: *mut crate::array::ArrayHeader,
    reason: f64,
) {
    use crate::array::{js_array_get_f64, js_array_set_f64};

    if result_promise.is_null() || state_arr.is_null() {
        return;
    }

    let rejected = js_array_get_f64(state_arr, 1);
    if rejected != 0.0 {
        return;
    }

    js_array_set_f64(state_arr, 1, 1.0);
    js_promise_reject(result_promise, reason);
}

/// Promise.race - takes an array of promises and returns a promise that resolves
/// or rejects with the first promise that settles.
#[no_mangle]
pub extern "C" fn js_promise_race(promises_arr: *const crate::array::ArrayHeader) -> *mut Promise {
    use crate::array::{js_array_get_f64, js_array_length};
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        // Promise.race([]) — never settles (per spec), but return pending promise
        return result_promise;
    }

    let count = js_array_length(promises_arr);
    if count == 0 {
        return result_promise;
    }

    // Both handlers capture only `result_promise` and don't depend on
    // the input index — so allocate once and share across all N inputs.
    // Saves (N-1) × 2 closure allocs per Promise.race call.
    let shared_resolve = js_closure_alloc(promise_race_resolve_handler as *const u8, 1);
    js_closure_set_capture_ptr(shared_resolve, 0, result_promise as i64);
    let shared_reject = js_closure_alloc(promise_race_reject_handler as *const u8, 1);
    js_closure_set_capture_ptr(shared_reject, 0, result_promise as i64);

    // Normalize each input with PromiseResolve semantics before attaching handlers.
    // This keeps plain values asynchronous and gives thenables a chance to settle
    // through the same guarded job path used by Promise.all/allSettled.
    for i in 0..count {
        let promise_ptr = match promise_resolve_for_combinator(js_array_get_f64(promises_arr, i)) {
            Ok(promise) => promise,
            Err(reason) => js_promise_rejected(reason),
        };
        js_promise_attach_handlers(promise_ptr, shared_resolve, shared_reject);
    }

    result_promise
}

/// Handler for Promise.race fulfill — resolves the race promise with the first value
extern "C" fn promise_race_resolve_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;
    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if result_promise.is_null() {
        return 0.0;
    }
    // Only settle if still pending (first one wins)
    if matches!(unsafe { (*result_promise).state }, PromiseState::Pending) {
        js_promise_resolve(result_promise, value);
    }
    0.0
}

/// Handler for Promise.race reject — rejects the race promise with the first reason
extern "C" fn promise_race_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;
    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if result_promise.is_null() {
        return 0.0;
    }
    if matches!(unsafe { (*result_promise).state }, PromiseState::Pending) {
        js_promise_reject(result_promise, reason);
    }
    0.0
}

/// Await any promise value.
/// In native-only mode (no V8), all promises are native POINTER_TAG promises.
/// The codegen-emitted busy-wait loop handles polling the promise state,
/// so we just return the value as-is.
/// In V8 mode (perry-jsruntime), this function is overridden by the V8-aware
/// version that can also handle JS_HANDLE_TAG promises.
#[no_mangle]
pub extern "C" fn js_await_any_promise(value: f64) -> f64 {
    value
}

/// ECMAScript thenable assimilation for `await`. Issue #586.
///
/// `await x` semantics: if `x` is an object with a callable `then` method,
/// the runtime should call `x.then(resolve, reject)` and resume with whatever
/// the underlying then implementation passes to `resolve`. Real Promises take
/// the fast path; thenables (e.g. drizzle-orm's `QueryPromise`) need this.
///
/// Behavior:
/// - Already a Promise → pass through unchanged (caller's await loop polls it).
/// - Object whose class chain contains a `then(onFulfilled, onRejected)` method
///   → allocate a fresh Promise, build resolve/reject closures bound to it,
///     invoke `value.then(resolve, reject)`, and return the new Promise (which
///     the await loop then polls). When the user's `then` calls `resolve(v)`,
///     our handler resolves the wrapper promise; the await loop sees Fulfilled
///     and returns `v`.
/// - Anything else (primitives, plain objects without a `then` method, Map /
///   Set / Buffer / handle values) → pass through unchanged so the await
///   resolves with the value itself per spec.
///
/// `then` is looked up only in the class vtable, not as an instance field.
/// Object literals with a `then: () => ...` arrow stored as a property are
/// uncommon in practice and would require a parallel `js_object_get_field_by_name`
/// probe — out of scope for this fix.
#[no_mangle]
pub extern "C" fn js_assimilate_thenable(value: f64) -> f64 {
    use crate::value::JSValue;

    bump(&MT_THENABLE_PROBE_COUNT);
    // Real Promise — caller's await loop already handles it.
    if js_value_is_promise(value) != 0 {
        return value;
    }

    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    if !jsval.is_pointer() {
        return value;
    }

    let raw_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if crate::value::addr_class::is_handle_band(raw_ptr) {
        return value;
    }

    // Side-table-tracked heap types don't have ClassVTable entries; skip.
    // Off-GC-heap header-less allocations (small typed arrays / buffers) come
    // first: they have no GcHeader, so they must be excluded before any probe
    // that back-reads `raw_ptr - GC_HEADER_SIZE` — including the
    // `is_date_cell_addr` call in this very chain (#5226).
    if crate::typedarray::is_offheap_sidetable_alloc(raw_ptr)
        || crate::set::is_registered_set(raw_ptr)
        || crate::map::is_registered_map(raw_ptr)
        || crate::symbol::is_registered_symbol(raw_ptr)
        || crate::regex::is_regex_pointer(raw_ptr as *const u8)
        || crate::date::is_date_cell_addr(raw_ptr)
    {
        return value;
    }

    let obj_ptr = jsval.as_pointer::<crate::object::ObjectHeader>();
    if obj_ptr.is_null() {
        return value;
    }

    // Verify GC type before reading class_id; reading garbage past random
    // pointers would either return a fake match or segfault.
    let class_id = unsafe {
        let gc_header =
            (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            return value;
        }
        (*obj_ptr).class_id
    };
    if class_id == 0 {
        // A plain object literal (`{ then(resolve, reject) {…} }` / `{ get then()
        // {…} }`) has no class vtable, so the method-chain probe below can't see
        // its `then`. Per ECMA-262 PromiseResolveThenableJob the thenable check
        // is `Get(value, "then")` + IsCallable — independent of how `then` is
        // stored. Route object literals through the property-based fallback so
        // `await { then(r){ r(v) } }` and delegated async-iterator results (whose
        // `next()` returns object-literal promises — test262 yield-star-async-*)
        // assimilate instead of resolving with the thenable object itself.
        return assimilate_via_then_property(value);
    }

    // Probe the vtable chain for `then` (a class *method*). If that fails, fall
    // back to reading `then` as an own/inherited DATA property — object-literal
    // thenables (`{ then(resolve, reject) { … } }`, the common test262 and real-
    // world shape) carry `then` as a plain property, not a class method, so the
    // vtable probe alone never assimilates them. Per ECMA-262 PromiseResolve /
    // PromiseResolveThenableJob: `thenAction = Get(value, "then")`; if callable,
    // run `Call(thenAction, value, «resolve, reject»)`. Otherwise resolve plain
    // (return the value unchanged).
    let (then_func_ptr, then_param_count, _then_has_synthetic_arguments, _then_has_rest) =
        match crate::object::lookup_class_method_in_chain(class_id, "then") {
            Some(p) => p,
            None => return assimilate_via_then_property(value),
        };

    // Allocate the wrapper promise plus resolve/reject closures pointing at it.
    let new_promise = js_promise_new();
    let promise_i64 = new_promise as i64;

    let resolve_closure = crate::closure::js_closure_alloc(promise_resolve_fn as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(resolve_closure, 0, promise_i64);
    let reject_closure = crate::closure::js_closure_alloc(promise_reject_fn as *const u8, 1);
    crate::closure::js_closure_set_capture_ptr(reject_closure, 0, promise_i64);

    // The user's `then(onFulfilled, onRejected)` reads each parameter as a
    // raw f64 closure pointer (matching the convention used by
    // `js_promise_new_with_executor`).
    let resolve_f64 = f64::from_bits(resolve_closure as u64);
    let reject_f64 = f64::from_bits(reject_closure as u64);

    // Invoke `value.then(resolve, reject)` via the vtable. Mirrors
    // `call_vtable_method` in object.rs: NaN-box `this` with POINTER_TAG so
    // the method body sees a real instance pointer.
    let this_f64 = f64::from_bits(JSValue::pointer(obj_ptr as *mut u8).bits());
    unsafe {
        match then_param_count {
            0 => {
                let f: extern "C" fn(f64) -> f64 = std::mem::transmute(then_func_ptr);
                f(this_f64);
            }
            1 => {
                let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(then_func_ptr);
                f(this_f64, resolve_f64);
            }
            _ => {
                // 2+ params: pass resolve/reject; any extra slots arrive as NaN.
                let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(then_func_ptr);
                f(this_f64, resolve_f64, reject_f64);
            }
        }
    }

    crate::value::js_nanbox_pointer(new_promise as i64)
}

/// Assimilate an object-literal thenable whose `then` is an own/inherited DATA
/// property (not a class method). Reads `Get(value, "then")`; if it is callable,

/// Build a `{ status: "fulfilled", value: v }` object for Promise.allSettled.
fn build_settled_fulfilled(value: f64) -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    let packed = b"status\0value\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF10, 2, packed.as_ptr(), packed.len() as u32);
    let status_str = crate::string::js_string_from_bytes(b"fulfilled".as_ptr(), 9);
    let status_nb = crate::value::js_nanbox_string(status_str as i64);
    js_object_set_field(
        obj,
        0,
        crate::value::JSValue::from_bits(status_nb.to_bits()),
    );
    js_object_set_field(obj, 1, crate::value::JSValue::from_bits(value.to_bits()));
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Build a `{ status: "rejected", reason: r }` object for Promise.allSettled.
fn build_settled_rejected(reason: f64) -> f64 {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    let packed = b"status\0reason\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF11, 2, packed.as_ptr(), packed.len() as u32);
    let status_str = crate::string::js_string_from_bytes(b"rejected".as_ptr(), 8);
    let status_nb = crate::value::js_nanbox_string(status_str as i64);
    js_object_set_field(
        obj,
        0,
        crate::value::JSValue::from_bits(status_nb.to_bits()),
    );
    js_object_set_field(obj, 1, crate::value::JSValue::from_bits(reason.to_bits()));
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Promise.allSettled — never rejects; resolves with an array of result objects
/// where each entry is `{ status: "fulfilled", value }` or `{ status: "rejected", reason }`.
#[no_mangle]
pub extern "C" fn js_promise_all_settled(
    promises_arr: *const crate::array::ArrayHeader,
) -> *mut Promise {
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};
    use crate::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        let empty_arr = js_array_alloc(0);
        unsafe {
            (*empty_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        promise_resolve_assimilating(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);
    if count == 0 {
        let empty_arr = js_array_alloc(0);
        unsafe {
            (*empty_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(empty_arr as i64);
        promise_resolve_assimilating(result_promise, arr_f64);
        return result_promise;
    }

    let results_arr = js_array_alloc(count);
    unsafe {
        (*results_arr).length = count;
    }
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(results_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    // state: [remaining_count]
    let state_arr = js_array_alloc(1);
    unsafe {
        (*state_arr).length = 1;
    }
    js_array_set_f64(state_arr, 0, count as f64);

    for i in 0..count {
        let promise_ptr = match promise_resolve_for_combinator(js_array_get_f64(promises_arr, i)) {
            Ok(promise) => promise,
            Err(reason) => {
                js_promise_reject(result_promise, reason);
                break;
            }
        };

        // Fulfill: store {status:"fulfilled", value:v}
        let fulfill_closure = js_closure_alloc(promise_all_settled_fulfill_handler as *const u8, 4);
        js_closure_set_capture_ptr(fulfill_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(fulfill_closure, 1, results_arr as i64);
        js_closure_set_capture_ptr(fulfill_closure, 2, state_arr as i64);
        js_closure_set_capture_f64(fulfill_closure, 3, i as f64);

        // Reject: store {status:"rejected", reason:r}
        let reject_closure = js_closure_alloc(promise_all_settled_reject_handler as *const u8, 4);
        js_closure_set_capture_ptr(reject_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(reject_closure, 1, results_arr as i64);
        js_closure_set_capture_ptr(reject_closure, 2, state_arr as i64);
        js_closure_set_capture_f64(reject_closure, 3, i as f64);

        js_promise_attach_handlers(promise_ptr, fulfill_closure, reject_closure);
    }

    // If all were already non-promises
    let remaining = js_array_get_f64(state_arr, 0);
    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        promise_resolve_assimilating(result_promise, arr_f64);
    }

    result_promise
}

extern "C" fn promise_all_settled_fulfill_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let results_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || results_arr.is_null() || state_arr.is_null() {
        return 0.0;
    }
    let index = js_closure_get_capture_f64(closure, 3) as u32;

    let wrapped = build_settled_fulfilled(value);
    js_array_set_f64(results_arr, index, wrapped);

    let remaining = js_array_get_f64(state_arr, 0) - 1.0;
    js_array_set_f64(state_arr, 0, remaining);

    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        promise_resolve_assimilating(result_promise, arr_f64);
    }
    0.0
}

extern "C" fn promise_all_settled_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let results_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || results_arr.is_null() || state_arr.is_null() {
        return 0.0;
    }
    let index = js_closure_get_capture_f64(closure, 3) as u32;

    let wrapped = build_settled_rejected(reason);
    js_array_set_f64(results_arr, index, wrapped);

    let remaining = js_array_get_f64(state_arr, 0) - 1.0;
    js_array_set_f64(state_arr, 0, remaining);

    if remaining == 0.0 {
        let arr_f64 = crate::value::js_nanbox_pointer(results_arr as i64);
        promise_resolve_assimilating(result_promise, arr_f64);
    }
    0.0
}

/// Promise.any — settles with the first FULFILLED promise. If all reject, rejects
/// with an `AggregateError` whose `errors` array carries the collected reasons
/// (constructed via `js_aggregate_error_new` in the all-rejected path below).
#[no_mangle]
pub extern "C" fn js_promise_any(promises_arr: *const crate::array::ArrayHeader) -> *mut Promise {
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_set_f64};
    use crate::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };

    let result_promise = js_promise_new();

    if promises_arr.is_null() {
        // Empty input — Promise.any rejects immediately with empty errors array
        let errors_arr = js_array_alloc(0);
        unsafe {
            (*errors_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(errors_arr as i64);
        js_promise_reject(result_promise, arr_f64);
        return result_promise;
    }

    let count = js_array_length(promises_arr);
    if count == 0 {
        let errors_arr = js_array_alloc(0);
        unsafe {
            (*errors_arr).length = 0;
        }
        let arr_f64 = crate::value::js_nanbox_pointer(errors_arr as i64);
        js_promise_reject(result_promise, arr_f64);
        return result_promise;
    }

    let errors_arr = js_array_alloc(count);
    unsafe {
        (*errors_arr).length = count;
    }
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    for i in 0..count {
        js_array_set_f64(errors_arr, i, f64::from_bits(TAG_UNDEFINED));
    }

    // state: [remaining_rejections, settled_flag]
    let state_arr = js_array_alloc(2);
    unsafe {
        (*state_arr).length = 2;
    }
    js_array_set_f64(state_arr, 0, count as f64);
    js_array_set_f64(state_arr, 1, 0.0);

    // Fulfill closure captures only `[result_promise, state_arr]` — no
    // per-index payload, so we share one across all N inputs (mirrors
    // the Promise.all reject-closure sharing in commit 7c89fcc6).
    // Reject still needs per-index since it must write its error into
    // the correct slot of `errors_arr` for the eventual AggregateError.
    let shared_fulfill = js_closure_alloc(promise_any_fulfill_handler as *const u8, 2);
    js_closure_set_capture_ptr(shared_fulfill, 0, result_promise as i64);
    js_closure_set_capture_ptr(shared_fulfill, 1, state_arr as i64);

    for i in 0..count {
        let promise_ptr = match promise_resolve_for_combinator(js_array_get_f64(promises_arr, i)) {
            Ok(promise) => promise,
            Err(reason) => js_promise_rejected(reason),
        };

        let reject_closure = js_closure_alloc(promise_any_reject_handler as *const u8, 4);
        js_closure_set_capture_ptr(reject_closure, 0, result_promise as i64);
        js_closure_set_capture_ptr(reject_closure, 1, errors_arr as i64);
        js_closure_set_capture_ptr(reject_closure, 2, state_arr as i64);
        js_closure_set_capture_f64(reject_closure, 3, i as f64);

        js_promise_attach_handlers(promise_ptr, shared_fulfill, reject_closure);
    }

    result_promise
}

extern "C" fn promise_any_fulfill_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::js_closure_get_capture_ptr;

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let state_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    if result_promise.is_null() || state_arr.is_null() {
        return 0.0;
    }

    let already_settled = js_array_get_f64(state_arr, 1);
    if already_settled != 0.0 {
        return 0.0;
    }
    js_array_set_f64(state_arr, 1, 1.0);

    js_promise_resolve(result_promise, value);
    0.0
}

extern "C" fn promise_any_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::array::{js_array_get_f64, js_array_set_f64, ArrayHeader};
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};

    let result_promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let errors_arr = js_closure_get_capture_ptr(closure, 1) as *mut ArrayHeader;
    let state_arr = js_closure_get_capture_ptr(closure, 2) as *mut ArrayHeader;
    if result_promise.is_null() || errors_arr.is_null() || state_arr.is_null() {
        return 0.0;
    }
    let index = js_closure_get_capture_f64(closure, 3) as u32;

    let already_settled = js_array_get_f64(state_arr, 1);
    if already_settled != 0.0 {
        return 0.0;
    }

    js_array_set_f64(errors_arr, index, reason);

    let remaining = js_array_get_f64(state_arr, 0) - 1.0;
    js_array_set_f64(state_arr, 0, remaining);

    if remaining == 0.0 {
        // All rejected — create an AggregateError with the collected
        // errors array and reject the result promise with it.
        js_array_set_f64(state_arr, 1, 1.0);
        let msg = crate::string::js_string_from_bytes(b"All promises were rejected".as_ptr(), 26);
        let agg_err = crate::error::js_aggregateerror_new(errors_arr, msg);
        let err_f64 = crate::value::js_nanbox_pointer(agg_err as i64);
        js_promise_reject(result_promise, err_f64);
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::{js_array_alloc, js_array_get_f64, js_array_set_f64};
    use crate::closure::{
        js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    };
    use crate::object::{js_object_alloc, js_object_get_field, js_object_set_field_by_name};
    use crate::value::js_nanbox_pointer;

    fn reset_promise_test_state() {
        TASK_QUEUE.with(|q| q.borrow_mut().clear());
        PROMISE_ALL_STATES.with(|s| s.borrow_mut().clear());
    }

    fn thenable_value(func: *const u8, captured: f64) -> f64 {
        let obj = js_object_alloc(0, 0);
        let then = js_closure_alloc(func, 1);
        js_closure_set_capture_f64(then, 0, captured);
        let key = crate::string::js_string_from_bytes(b"then".as_ptr(), 4);
        js_object_set_field_by_name(obj, key, js_nanbox_pointer(then as i64));
        js_nanbox_pointer(obj as i64)
    }

    #[test]
    fn promise_probe_rejects_pointer_tagged_native_handles() {
        let fetch_family_handle = js_nanbox_pointer(0x40001);
        assert_eq!(js_value_is_promise(fetch_family_handle), 0);
    }

    // #5226: small typed arrays / buffers are system-`alloc`'d off the GC heap
    // with NO 8-byte GcHeader prefix and are tracked only in side tables. The
    // type-dispatch probes (`js_value_is_promise`, `is_date_cell_addr`, …) must
    // recognize them via the side table and must NOT back-read
    // `ptr - GC_HEADER_SIZE` — a read on a block at the start of a freshly
    // mapped region crosses into the (unmapped) preceding page and segfaults.
    // Reproduce that worst case with a guarded mapping: without the side-table
    // skip these probes SIGSEGV; with it, they classify the block correctly.
    #[cfg(unix)]
    #[test]
    fn type_probes_skip_offheap_typed_array_with_unmapped_preceding_page() {
        unsafe {
            let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
            let total = page * 2;
            let base = libc::mmap(
                std::ptr::null_mut(),
                total,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            assert_ne!(base, libc::MAP_FAILED);
            // Guard the first page so any `ptr - GC_HEADER_SIZE` back-read faults.
            assert_eq!(libc::mprotect(base, page, libc::PROT_NONE), 0);
            let ta = (base as *mut u8).add(page) as *const crate::typedarray::TypedArrayHeader;
            crate::typedarray::register_typed_array(ta, 0);

            let value = js_nanbox_pointer(ta as i64);
            assert_eq!(js_value_is_promise(value), 0);
            assert!(!crate::date::is_date_cell_addr(ta as usize));

            crate::typedarray::unregister_typed_array(ta);
            assert_eq!(libc::munmap(base, total), 0);
        }
    }

    extern "C" fn test_thenable_resolve_twice(
        closure: *const crate::closure::ClosureHeader,
        on_fulfilled: f64,
        _on_rejected: f64,
    ) -> f64 {
        let value = js_closure_get_capture_f64(closure, 0);
        let unexpected = value + 1000.0;
        unsafe {
            crate::closure::js_native_call_value(on_fulfilled, [value].as_ptr(), 1);
            crate::closure::js_native_call_value(on_fulfilled, [unexpected].as_ptr(), 1);
        }
        0.0
    }

    extern "C" fn test_thenable_reject(
        closure: *const crate::closure::ClosureHeader,
        _on_fulfilled: f64,
        on_rejected: f64,
    ) -> f64 {
        let reason = js_closure_get_capture_f64(closure, 0);
        unsafe {
            crate::closure::js_native_call_value(on_rejected, [reason].as_ptr(), 1);
        }
        0.0
    }

    #[test]
    fn promise_all_assimilates_thenable_and_guards_double_resolve() {
        unsafe {
            reset_promise_test_state();
            let arr = js_array_alloc(1);
            (*arr).length = 1;
            js_array_set_f64(
                arr,
                0,
                thenable_value(test_thenable_resolve_twice as *const u8, 7.0),
            );

            let all = js_promise_all(arr);
            assert_eq!((*all).state, PromiseState::Pending);

            crate::promise::js_promise_run_microtasks();

            assert_eq!((*all).state, PromiseState::Fulfilled);
            let results = crate::value::js_nanbox_get_pointer((*all).value)
                as *const crate::array::ArrayHeader;
            assert_eq!(js_array_get_f64(results, 0), 7.0);
        }
    }

    #[test]
    fn promise_all_rejects_from_thenable_job() {
        unsafe {
            reset_promise_test_state();
            let arr = js_array_alloc(1);
            (*arr).length = 1;
            js_array_set_f64(
                arr,
                0,
                thenable_value(test_thenable_reject as *const u8, 13.0),
            );

            let all = js_promise_all(arr);
            assert_eq!((*all).state, PromiseState::Pending);

            crate::promise::js_promise_run_microtasks();

            assert_eq!((*all).state, PromiseState::Rejected);
            assert_eq!((*all).reason, 13.0);
        }
    }

    #[test]
    fn promise_all_settled_assimilates_thenables_in_input_order() {
        unsafe {
            reset_promise_test_state();
            let arr = js_array_alloc(2);
            (*arr).length = 2;
            js_array_set_f64(
                arr,
                0,
                thenable_value(test_thenable_reject as *const u8, 1.0),
            );
            js_array_set_f64(
                arr,
                1,
                thenable_value(test_thenable_resolve_twice as *const u8, 2.0),
            );

            let settled = js_promise_all_settled(arr);
            assert_eq!((*settled).state, PromiseState::Pending);

            crate::promise::js_promise_run_microtasks();

            assert_eq!((*settled).state, PromiseState::Fulfilled);
            let results = crate::value::js_nanbox_get_pointer((*settled).value)
                as *const crate::array::ArrayHeader;
            let first = crate::value::js_nanbox_get_pointer(js_array_get_f64(results, 0))
                as *const crate::object::ObjectHeader;
            let second = crate::value::js_nanbox_get_pointer(js_array_get_f64(results, 1))
                as *const crate::object::ObjectHeader;
            assert_eq!(js_object_get_field(first, 1).bits(), 1.0f64.to_bits());
            assert_eq!(js_object_get_field(second, 1).bits(), 2.0f64.to_bits());
        }
    }

    #[test]
    fn promise_result_resolution_assimilates_thenable_objects() {
        unsafe {
            reset_promise_test_state();
            let promise = js_promise_new();
            promise_resolve_assimilating(
                promise,
                thenable_value(test_thenable_resolve_twice as *const u8, 21.0),
            );
            assert_eq!((*promise).state, PromiseState::Pending);

            crate::promise::js_promise_run_microtasks();

            assert_eq!((*promise).state, PromiseState::Fulfilled);
            assert_eq!((*promise).value, 21.0);
        }
    }

    /// Regression: a pending promise reused as input to two `Promise.all`
    /// calls must settle BOTH all-promises when it resolves. Pre-fix,
    /// `promise_all_take_handler` only popped the first matching state
    /// (via `swap_remove`), so the second `Promise.all` hung forever.
    #[test]
    fn promise_all_with_shared_pending_input_resolves_both() {
        unsafe {
            // Pending promise that will be shared across two Promise.all calls.
            let shared = js_promise_new();

            // Second input for each all() — a pre-resolved promise so the
            // remaining counter only needs `shared` to settle.
            let other_a = js_promise_new();
            js_promise_resolve(other_a, 100.0);
            let other_b = js_promise_new();
            js_promise_resolve(other_b, 200.0);

            // Build [shared, other_a]
            let arr_a = js_array_alloc(2);
            (*arr_a).length = 2;
            js_array_set_f64(arr_a, 0, js_nanbox_pointer(shared as i64));
            js_array_set_f64(arr_a, 1, js_nanbox_pointer(other_a as i64));
            let all_a = js_promise_all(arr_a);

            // Build [shared, other_b]
            let arr_b = js_array_alloc(2);
            (*arr_b).length = 2;
            js_array_set_f64(arr_b, 0, js_nanbox_pointer(shared as i64));
            js_array_set_f64(arr_b, 1, js_nanbox_pointer(other_b as i64));
            let all_b = js_promise_all(arr_b);

            // Both all() results should still be pending.
            assert_eq!((*all_a).state, PromiseState::Pending);
            assert_eq!((*all_b).state, PromiseState::Pending);

            // PROMISE_ALL_STATES must hold TWO entries keyed on `shared`.
            let registered = PROMISE_ALL_STATES.with(|s| {
                s.borrow()
                    .iter()
                    .filter(|(k, _)| *k == shared as usize)
                    .count()
            });
            assert_eq!(
                registered, 2,
                "expected two Promise.all states keyed on the shared pending promise"
            );

            // Settle the shared promise; drain microtasks so PromiseAll tasks
            // run and update both result arrays.
            js_promise_resolve(shared, 42.0);
            crate::promise::js_promise_run_microtasks();

            // Both Promise.all results must now be Fulfilled.
            assert_eq!(
                (*all_a).state,
                PromiseState::Fulfilled,
                "first Promise.all should have settled"
            );
            assert_eq!(
                (*all_b).state,
                PromiseState::Fulfilled,
                "second Promise.all should have settled (was hanging pre-fix)"
            );
        }
    }
}
