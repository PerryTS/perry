//! `class X extends Promise` — subclass backing support.
//!
//! Perry models a class instance as a plain `ObjectHeader`, not a real
//! `GC_TYPE_PROMISE` cell (a Promise carries its state in a dedicated
//! allocation, not in object fields). So `super(executor)` to a `Promise`
//! parent used to be a best-effort no-op, leaving the subclass instance with no
//! promise state — `inst.then(...)` threw "incompatible receiver" and
//! `NewPromiseCapability(Subclass)` never populated `resolve`/`reject`.
//!
//! Fix (mirrors `object::map_set_subclass`): `super(executor)` calls
//! [`js_promise_subclass_init`], which runs the ECMA-262 27.2.3.1 Promise
//! constructor steps against a fresh backing `Promise` cell — CreateResolving
//! Functions + `Call(executor, «resolve, reject»)` — and stashes the backing
//! cell's NaN-boxed pointer on the instance under a hidden field. Because it is
//! a normal object field, the GC traces + relocates it.
//!
//! `inst.then/catch/finally`, `await inst`, and `js_value_is_promise(inst)` are
//! then served by unwrapping the backing cell via [`subclass_backing_promise`]
//! at the runtime dispatch points.

use crate::object::{js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader};
use crate::value::{JSValue, POINTER_MASK};

use super::Promise;

/// Hidden field on a Promise subclass instance holding the NaN-boxed backing
/// `Promise` cell pointer.
pub(crate) const BACKING_KEY: &[u8] = b"__perry_promise_backing__";

/// Has any `class X extends Promise` instance EVER stashed a backing cell in
/// this process? `subclass_backing_promise` costs a key-string alloc plus a
/// full recursive `js_object_get_field_by_name_f64` per call, and it is
/// consulted from `js_object_get_field_by_name`'s miss path — which every
/// `await` of a plain object reaches via the spec thenable check
/// (`Get(v, "then")`). A program that never subclasses `Promise` should pay a
/// relaxed load, not an allocation. Set at the single stash site
/// (`js_promise_subclass_init`, the only writer of `BACKING_KEY`). (#7795)
pub(crate) static PROMISE_SUBCLASS_EVER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() {
        return (bits & POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn instance_object_ptr(this: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(this);
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    // A real `Promise` cell (GC_TYPE_PROMISE) is not a subclass instance — it
    // has its own dispatch. Reject registered non-object allocations before the
    // header read for the same magnitude/handle-band reasons as the Map/Set path.
    if crate::map::is_registered_map(raw)
        || crate::set::is_registered_set(raw)
        || crate::buffer::is_registered_buffer(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
    {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(raw)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    Some(raw as *mut ObjectHeader)
}

/// If `value` is a Promise *subclass instance* (a plain object carrying the
/// hidden backing field), return its backing `Promise` cell. Returns `None` for
/// real `Promise` cells, ordinary objects, and non-objects — so callers fall
/// through to their existing handling.
pub(crate) fn subclass_backing_promise(value: f64) -> Option<*mut Promise> {
    // #7795: no `class X extends Promise` instance exists, so this cannot
    // return `Some`. Answer from the monotone flag.
    if !PROMISE_SUBCLASS_EVER.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    unsafe {
        let obj = instance_object_ptr(value)?;
        let backing = js_object_get_field_by_name_f64(
            obj as *const ObjectHeader,
            crate::string::js_string_from_bytes(BACKING_KEY.as_ptr(), BACKING_KEY.len() as u32),
        );
        let bjs = JSValue::from_bits(backing.to_bits());
        if !bjs.is_pointer() {
            return None;
        }
        let raw = (backing.to_bits() & POINTER_MASK) as usize;
        if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return None;
        }
        // Confirm the stashed pointer is a genuine Promise cell before handing it
        // back as one.
        if crate::promise::js_value_is_promise(f64::from_bits(bjs.bits())) != 0 {
            return Some(raw as *mut Promise);
        }
        None
    }
}

/// `super(executor)` for a `class X extends Promise`. Runs ECMA-262 27.2.3.1
/// (Promise constructor): allocate a backing `Promise` cell, create its
/// resolving functions, call `executor(resolve, reject)` (catching an abrupt
/// completion into a rejection), and stash the backing cell on the instance
/// under the hidden field. `executor` is the (required-callable) first
/// constructor argument; a non-callable executor throws a `TypeError`
/// synchronously per step 2.
#[no_mangle]
pub extern "C" fn js_promise_subclass_init(this: f64, executor: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let executor = scope.root_nanbox_f64(executor);
    if unsafe { instance_object_ptr(this.get_nanbox_f64()) }.is_none() {
        return this.get_nanbox_f64();
    }

    // 27.2.3.1 step 2: a non-callable executor throws a TypeError, before any
    // promise is created.
    if !super::spec_combinators::is_callable_value(executor.get_nanbox_f64()) {
        let msg = b"Promise resolver is not a function";
        let s = scope.root_string_ptr(crate::string::js_string_from_bytes(
            msg.as_ptr(),
            msg.len() as u32,
        ));
        let err = s.with_mut_ptr::<crate::StringHeader, _>(|s| crate::error::js_typeerror_new(s));
        let v = f64::from_bits(JSValue::pointer(err as *const u8).bits());
        crate::exception::js_throw(v);
    }

    // Build the backing promise + resolving functions, run the executor. Keep a
    // raw root on the backing cell across the string-key allocation below (which
    // can GC) by stashing it immediately after the executor runs.
    let promise = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        super::js_promise_new() as i64
    ));
    let promise_ptr = crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise;
    let (resolve_closure, reject_closure) =
        super::combinators::make_resolving_functions(promise_ptr);
    let resolve = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(resolve_closure as i64));
    let reject = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(reject_closure as i64));

    // 27.2.3.1 step 10: run the executor; a throw rejects the promise via the
    // shared resolving `reject` (so the [[AlreadyResolved]] guard makes a later
    // resolve/reject a no-op). `js_native_call_value` accepts both POINTER_TAG
    // closures and raw-pointer-bits closures, so `executor` is passed as-is.
    let args = [resolve.get_nanbox_f64(), reject.get_nanbox_f64()];
    if let Err(reason) = super::combinators::combinator_catch_js(|| unsafe {
        crate::closure::js_native_call_value(executor.get_nanbox_f64(), args.as_ptr(), args.len())
    }) {
        let reject = crate::value::js_nanbox_get_pointer(reject.get_nanbox_f64())
            as *const crate::closure::ClosureHeader;
        crate::closure::js_closure_call1(reject, reason);
    }

    // #7795: arm the probe gate before the field exists, so no reader can
    // observe a stashed backing cell while the flag still says "never".
    PROMISE_SUBCLASS_EVER.store(true, std::sync::atomic::Ordering::Relaxed);
    let key = crate::string::js_string_from_bytes(BACKING_KEY.as_ptr(), BACKING_KEY.len() as u32);
    let obj = unsafe { instance_object_ptr(this.get_nanbox_f64()) }
        .expect("rooted Promise subclass receiver must remain an object");
    let backing_bits = promise.get_nanbox_f64().to_bits();
    js_object_set_field_by_name(obj, key, f64::from_bits(backing_bits));
    this.get_nanbox_f64()
}
