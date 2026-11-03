//! Iterator helper objects (ES2025 §27.1.2.1, Iterator Helpers; issue #2874).
//!
//! `map`/`filter`/`take`/`drop`/`flatMap` return lazy helper objects that
//! inherit from `%IteratorHelperPrototype%`; the methods themselves live on
//! `%Iterator.prototype%` ([`proto_methods`]).
//!
//! Representation mirrors `collection_iter_object.rs`: a regular `ObjectHeader`
//! with a dedicated class id (`ITERATOR_HELPER_CLASS_ID`) and the fields
//! below. An `OP_IDENTITY` object is an Iterator Record (GetIteratorDirect):
//! the iterator plus the `next` read once. Every other op steps its
//! underlying iterator through such a record, or directly through a helper
//! whose `next` is the intrinsic one, without allocating result objects.

use crate::closure::{is_closure_ptr, ClosureHeader, JsThis};
// #7564: `make_iter_result` used to be a local five-allocation copy whose
// intermediates were all bare Rust locals. It was the copy that faulted under
// `PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_PROTECT_FROMSPACE=1`
// — `make_iter_result + 188`, a
// retired from-space object — and it now comes from the one rooted,
// shared-shape constructor in `crate::iter_result`.
use crate::iter_result::make_iter_result;
use crate::object::{
    js_object_alloc, js_object_get_field, js_object_get_field_by_name, js_object_set_field,
    ObjectHeader,
};
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_UNDEFINED};
use std::cell::Cell;

mod proto_methods;
mod statics;
pub(crate) use proto_methods::install_prototype_methods;
pub use proto_methods::{
    dispatch_iterator_helper_method, is_iterator_helper_method,
    maybe_dispatch_helper_on_builtin_iterator, maybe_dispatch_helper_on_iterator,
};
pub use statics::js_iterator_from;
pub(crate) use statics::{
    install_dispose, install_iterator_statics, install_wrap_prototype_methods,
};

#[cfg(test)]
mod tests;

/// Class id reserved for lazy iterator-helper objects.
///
/// #7576: this was `0xFFFF_0009`, which is [`STRING_ITERATOR_CLASS_ID`]. The
/// two constants were introduced independently, each with a comment reading
/// "sits just past the Set iterator id (0xFFFF0008)", and neither author
/// checked whether the slot was taken. Every dispatch tower tests the String
/// arm before the helper arm, so EVERY helper object was dispatched as a
/// String iterator: `.next()` read the helper's op-kind field as a cursor
/// index against a null backing array and answered `{ done: true }` on the
/// first step, and every other helper method (`map`/`filter`/`take`/`drop`/
/// `flatMap`/`toArray`/`reduce`/…) fell into that dispatcher's `_ =>
/// undefined` arm. The whole proposal surface was dead — silently for
/// `Iterator.from`, loudly (`Cannot read properties of undefined`) for the
/// combinators.
///
/// `0xFFFF_000A` is the RegExp-string iterator, so this takes the next free
/// slot. [`iterator_class_ids_are_pairwise_distinct`] pins the whole family so
/// a third collision cannot be introduced silently.
///
/// [`STRING_ITERATOR_CLASS_ID`]: crate::string::STRING_ITERATOR_CLASS_ID
/// [`iterator_class_ids_are_pairwise_distinct`]: tests::iterator_class_ids_are_pairwise_distinct
pub const ITERATOR_HELPER_CLASS_ID: u32 = 0xFFFF_000B;

// Op kinds stored in `F_OP`.
const OP_IDENTITY: i32 = 0; // Iterator Record (GetIteratorDirect).
const OP_MAP: i32 = 1;
const OP_FILTER: i32 = 2;
const OP_TAKE: i32 = 3;
const OP_DROP: i32 = 4;
const OP_FLATMAP: i32 = 5;
/// An `Iterator.from` wrapper: an Iterator Record with its own prototype.
const OP_WRAP: i32 = 6;
const OP_CONCAT: i32 = 7;

/// Identity: the iterator itself. Others: the record or helper they step.
const F_ITERATED: u32 = 0;
const F_OP: u32 = 1;
/// The callback (map/filter/flatMap), the validated limit (take/drop) or
/// concat's `[item, @@iterator method, ...]` array.
const F_ARG: u32 = 2;
/// Remaining count (take/drop) or the active inner record (flatMap/concat).
const F_STATE: u32 = 3;
/// Identity: the `next` method read once by GetIteratorDirect.
const F_NEXT: u32 = 4;
/// Callback counter, or concat's next item index.
const F_COUNTER: u32 = 5;
const F_GEN_STATE: u32 = 6;
pub(crate) const FIELD_COUNT: u32 = 7;

// `[[GeneratorState]]` of a helper (§27.1.2.1).
const GEN_START: f64 = 0.0;
const GEN_YIELD: f64 = 1.0;
const GEN_RUNNING: f64 = 2.0;
const GEN_DONE: f64 = 3.0;

pub fn is_iterator_helper_addr(addr: usize) -> bool {
    if addr < 0x1000 || (addr as u64) >> 48 != 0 {
        return false;
    }
    unsafe {
        let gc_header =
            (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if crate::gc_type_is_not_object!((*gc_header).obj_type) {
            return false;
        }
        (*(addr as *const ObjectHeader)).class_id == ITERATOR_HELPER_CLASS_ID
    }
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

/// ECMAScript "is an Object" (functions included).
pub(crate) fn is_object(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_pointer()
}

fn is_nullish(value: f64) -> bool {
    let v = JSValue::from_bits(value.to_bits());
    v.is_undefined() || v.is_null()
}

fn throw_error(range: bool, message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = if range {
        crate::error::js_rangeerror_new(msg)
    } else {
        crate::error::js_typeerror_new(msg)
    };
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

pub(crate) fn type_error(message: &str) -> ! {
    throw_error(false, message)
}

pub(crate) fn range_error(message: &str) -> ! {
    throw_error(true, message)
}

unsafe fn field(obj: f64, index: u32) -> f64 {
    let p = js_nanbox_get_pointer(obj) as *const ObjectHeader;
    f64::from_bits(js_object_get_field(p, index).bits())
}

unsafe fn set_field(obj: f64, index: u32, value: f64) {
    let p = js_nanbox_get_pointer(obj) as *mut ObjectHeader;
    js_object_set_field(p, index, JSValue::from_bits(value.to_bits()));
}

/// `Get(obj, name)`.
pub(crate) unsafe fn get(obj: f64, name: &'static [u8]) -> f64 {
    let key = crate::string::intern_ascii_literal(name);
    let key = crate::js_nanbox_string(key as i64);
    crate::proxy::js_reflect_get(obj, key, obj)
}

/// `Call(f, this, args)`.
unsafe fn call(f: f64, this: f64, args: &[f64]) -> f64 {
    let ptr = if args.is_empty() {
        std::ptr::null()
    } else {
        args.as_ptr()
    };
    crate::closure::native_call_value_this(f, JsThis::from_f64(this), ptr, args.len())
}

/// `Call(callback, undefined, args)`, direct for plain closures.
unsafe fn call_back(callback: f64, args: &[f64]) -> f64 {
    let p = js_nanbox_get_pointer(callback) as *const ClosureHeader;
    if !p.is_null() && is_closure_ptr(p as usize) {
        let recv = crate::closure::plain_call_receiver();
        match *args {
            [a, b] => return crate::closure::js_closure_call2(p, recv, a, b),
            [a, b, c] => return crate::closure::js_closure_call3(p, recv, a, b, c),
            _ => {}
        }
    }
    call(callback, undefined(), args)
}

fn truthy(value: f64) -> bool {
    crate::value::js_is_truthy(value) != 0
}

/// IteratorStepValue on `(iter, next)`; `None` when done.
///
/// #7564: the keys are interned (allocation-free after the first call per
/// thread) and every pointer is re-read from a handle after each call that
/// can collect, so no pre-collection address is nameable.
unsafe fn step_value(iter: f64, next: f64) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter_h = scope.root_nanbox_f64(iter);
    let result = if JSValue::from_bits(next.to_bits()).is_undefined() {
        // A Perry-native iterator whose `next` is not a readable property.
        crate::object::js_native_call_method(
            iter_h.get_nanbox_f64(),
            b"next".as_ptr() as *const i8,
            4,
            std::ptr::null(),
            0,
        )
    } else {
        if !crate::object::value_is_callable(next) {
            type_error("Iterator next method is not a function");
        }
        call(next, iter_h.get_nanbox_f64(), &[])
    };
    if !is_object(result) {
        type_error("Iterator result is not an object");
    }
    // `js_object_get_field_by_name` (getters, shape work) can collect between
    // the two reads, so the second read does not reuse the first's address.
    let result_h = scope.root_nanbox_f64(result);
    let (done_key, result_now) =
        result_h.across_nanbox(|| crate::string::intern_ascii_literal(b"done"));
    let done = js_object_get_field_by_name(
        js_nanbox_get_pointer(result_now) as *const ObjectHeader,
        done_key,
    );
    if truthy(f64::from_bits(done.bits())) {
        return None;
    }
    let (value_key, result_now) =
        result_h.across_nanbox(|| crate::string::intern_ascii_literal(b"value"));
    let value = js_object_get_field_by_name(
        js_nanbox_get_pointer(result_now) as *const ObjectHeader,
        value_key,
    );
    Some(f64::from_bits(value.bits()))
}

/// Allocate a helper. `iterated` and `arg` are rooted across the birth.
unsafe fn alloc_helper(op: i32, iterated: f64, arg: f64, next: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iterated = scope.root_nanbox_f64(iterated);
    let arg = scope.root_nanbox_f64(arg);
    let next = scope.root_nanbox_f64(next);
    let obj_h = scope.root_raw_mut_ptr(js_object_alloc(ITERATOR_HELPER_CLASS_ID, FIELD_COUNT));
    let obj = js_nanbox_pointer(obj_h.get_raw_mut_ptr::<ObjectHeader>() as i64);
    set_field(obj, F_ITERATED, iterated.get_nanbox_f64());
    set_field(obj, F_OP, op as f64);
    set_field(obj, F_ARG, arg.get_nanbox_f64());
    let state = match op {
        OP_TAKE | OP_DROP => arg.get_nanbox_f64(),
        _ => undefined(),
    };
    set_field(obj, F_STATE, state);
    set_field(obj, F_NEXT, next.get_nanbox_f64());
    set_field(obj, F_COUNTER, 0.0);
    set_field(obj, F_GEN_STATE, GEN_START);
    if op == OP_WRAP {
        crate::object::attach_wrap_for_valid_iterator_prototype(obj_h.get_raw_mut_ptr());
    } else {
        // `%IteratorHelperPrototype%`, whose parent is `%Iterator.prototype%`.
        crate::object::attach_iterator_prototype(obj_h.get_raw_mut_ptr(), ITERATOR_HELPER_CLASS_ID);
    }
    js_nanbox_pointer(obj_h.get_raw_mut_ptr::<ObjectHeader>() as i64)
}

/// A new helper of `op` over `iterated` (a record or helper).
pub(crate) unsafe fn new_helper(op: i32, iterated: f64, arg: f64) -> f64 {
    alloc_helper(op, iterated, arg, undefined())
}

/// GetIteratorDirect(obj) (§27.1.1.3): `next` is read once. A helper or
/// wrapper whose `next` is the intrinsic one is stepped directly (a wrapper's
/// `next` only forwards to its record); anything else gets an identity record.
pub(crate) unsafe fn get_iterator_direct(obj: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(obj);
    let next = get(obj, b"next");
    let p = js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as usize;
    let np = js_nanbox_get_pointer(next) as *const ClosureHeader;
    if is_iterator_helper_addr(p) && !np.is_null() && is_closure_ptr(np as usize) {
        let intrinsic = if field(obj_h.get_nanbox_f64(), F_OP) as i32 == OP_WRAP {
            statics::wrap_next_code()
        } else {
            crate::object::iterator_prototypes::iterator_helper_next_code()
        };
        if (*np).code() == intrinsic {
            return obj_h.get_nanbox_f64();
        }
    }
    alloc_helper(OP_IDENTITY, obj_h.get_nanbox_f64(), undefined(), next)
}

/// The iterator a record or helper closes: the wrapped one for a record (a
/// wrapper closes through its own `return`).
unsafe fn close_target(iterated: f64) -> f64 {
    if field(iterated, F_OP) as i32 == OP_IDENTITY {
        field(iterated, F_ITERATED)
    } else {
        iterated
    }
}

/// IteratorClose(iterated, normal completion): `return` errors propagate.
pub(crate) unsafe fn close_normal(iterated: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(close_target(iterated));
    let ret = get(target.get_nanbox_f64(), b"return");
    if is_nullish(ret) {
        return;
    }
    if !crate::object::value_is_callable(ret) {
        type_error("Iterator return method is not a function");
    }
    if !is_object(call(ret, target.get_nanbox_f64(), &[])) {
        type_error("Iterator result is not an object");
    }
}

/// IteratorClose(iterated, throw completion): the original error wins.
pub(crate) unsafe fn close_quietly(iterated: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(close_target(iterated));
    let _ = crate::exception::catch_js_throw(|| {
        let ret = get(target.get_nanbox_f64(), b"return");
        if crate::object::value_is_callable(ret) {
            call(ret, target.get_nanbox_f64(), &[]);
        }
    });
}

/// GetIteratorFlattenable(value, reject-primitives) as a record.
unsafe fn get_iterator_flattenable(value: f64) -> f64 {
    get_iterator_direct(statics::flattenable_iterator(value, false))
}

/// Step a record or helper; `None` when done. Records have no state; a
/// helper runs as a generator (§27.1.2.1): re-entry throws, a completion or
/// throw leaves it done, and a callback's throw closes its iterator.
pub(crate) unsafe fn helper_step(iterated: f64) -> Option<f64> {
    let op = field(iterated, F_OP) as i32;
    if op == OP_IDENTITY || op == OP_WRAP {
        return step_value(field(iterated, F_ITERATED), field(iterated, F_NEXT));
    }
    let state = field(iterated, F_GEN_STATE);
    if state == GEN_RUNNING {
        type_error("Generator is already running");
    }
    if state == GEN_DONE {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let helper = scope.root_nanbox_f64(iterated);
    set_field(iterated, F_GEN_STATE, GEN_RUNNING);
    let close = Cell::new(false);
    match crate::exception::catch_js_throw(|| run_op(&helper, op, &close)) {
        Ok(value) => {
            let state = if value.is_some() { GEN_YIELD } else { GEN_DONE };
            set_field(helper.get_nanbox_f64(), F_GEN_STATE, state);
            value
        }
        Err(error) => {
            let error = scope.root_nanbox_f64(error);
            set_field(helper.get_nanbox_f64(), F_GEN_STATE, GEN_DONE);
            if close.get() {
                close_quietly(field(helper.get_nanbox_f64(), F_ITERATED));
            }
            crate::exception::js_throw(error.get_nanbox_f64())
        }
    }
}

/// One step of a non-identity helper. `close` is set while an abrupt
/// completion must close the underlying iterator (IfAbruptCloseIterator).
unsafe fn run_op(
    helper: &crate::gc::RuntimeHandle<'_>,
    op: i32,
    close: &Cell<bool>,
) -> Option<f64> {
    let h = || helper.get_nanbox_f64();
    let iterated = || field(h(), F_ITERATED);
    let next_counter = || {
        let counter = field(h(), F_COUNTER);
        set_field(h(), F_COUNTER, counter + 1.0);
        counter
    };
    match op {
        OP_MAP => {
            let value = helper_step(iterated())?;
            close.set(true);
            let counter = next_counter();
            Some(call_back(field(h(), F_ARG), &[value, counter]))
        }
        OP_FILTER => loop {
            close.set(false);
            let value = helper_step(iterated())?;
            close.set(true);
            let scope = crate::gc::RuntimeHandleScope::new();
            let value = scope.root_nanbox_f64(value);
            let counter = next_counter();
            if truthy(call_back(
                field(h(), F_ARG),
                &[value.get_nanbox_f64(), counter],
            )) {
                return Some(value.get_nanbox_f64());
            }
        },
        OP_TAKE => {
            let remaining = field(h(), F_STATE);
            if remaining == 0.0 {
                close_normal(iterated());
                return None;
            }
            // Infinity stays Infinity.
            set_field(h(), F_STATE, remaining - 1.0);
            helper_step(iterated())
        }
        OP_DROP => {
            let mut remaining = field(h(), F_STATE);
            while remaining > 0.0 {
                remaining -= 1.0;
                set_field(h(), F_STATE, remaining);
                if helper_step(iterated()).is_none() {
                    set_field(h(), F_STATE, 0.0);
                    return None;
                }
            }
            helper_step(iterated())
        }
        OP_FLATMAP => loop {
            let inner = field(h(), F_STATE);
            if is_object(inner) {
                close.set(true);
                if let Some(value) = helper_step(inner) {
                    return Some(value);
                }
                set_field(h(), F_STATE, undefined());
            }
            close.set(false);
            let value = helper_step(iterated())?;
            close.set(true);
            let counter = next_counter();
            let mapped = call_back(field(h(), F_ARG), &[value, counter]);
            let inner = get_iterator_flattenable(mapped);
            set_field(h(), F_STATE, inner);
        },
        // A step error is not closed: only the active inner iterator is open.
        OP_CONCAT => loop {
            let inner = field(h(), F_STATE);
            if is_object(inner) {
                if let Some(value) = helper_step(inner) {
                    return Some(value);
                }
                set_field(h(), F_STATE, undefined());
            }
            let index = field(h(), F_COUNTER) as u32;
            let list = js_nanbox_get_pointer(field(h(), F_ARG)) as *const crate::array::ArrayHeader;
            if index * 2 >= crate::array::js_array_length(list) {
                return None;
            }
            set_field(h(), F_COUNTER, (index + 1) as f64);
            let item = crate::array::js_array_get_f64(list, index * 2);
            let method = crate::array::js_array_get_f64(list, index * 2 + 1);
            let iter = call(method, item, &[]);
            if !is_object(iter) {
                type_error("Iterator is not an object");
            }
            set_field(h(), F_STATE, get_iterator_direct(iter));
        },
        _ => None,
    }
}

/// `%IteratorHelperPrototype%.next` (§27.1.2.1.1).
unsafe fn helper_next(obj: *mut ObjectHeader) -> f64 {
    if !is_helper_op(js_nanbox_pointer(obj as i64)) {
        type_error("Method Iterator Helper.prototype.next called on incompatible receiver");
    }
    match helper_step(js_nanbox_pointer(obj as i64)) {
        Some(value) => make_iter_result(JSValue::from_bits(value.to_bits()), false),
        None => make_iter_result(JSValue::undefined(), true),
    }
}

/// Run the intrinsic `Iterator Helper.prototype.next` algorithm without
/// consulting instance or prototype overrides. The canonical prototype thunk
/// uses this entry point so a saved `helper.next.bind(helper)` keeps advancing
/// the helper it captured even if user code later replaces a `next` property.
pub(crate) unsafe fn iterator_helper_next_builtin(obj: *mut ObjectHeader) -> f64 {
    helper_next(obj)
}

/// Is the helper-class object `obj` a helper (has `[[UnderlyingIterators]]`),
/// not a record or an `Iterator.from` wrapper?
pub(crate) unsafe fn is_helper_op(obj: f64) -> bool {
    !matches!(field(obj, F_OP) as i32, OP_IDENTITY | OP_WRAP)
}

/// `%IteratorHelperPrototype%.return` (§27.1.2.1.2).
pub(crate) unsafe fn helper_return(obj: *mut ObjectHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let helper = scope.root_nanbox_f64(js_nanbox_pointer(obj as i64));
    let h = || helper.get_nanbox_f64();
    let done = || make_iter_result(JSValue::undefined(), true);
    let op = field(h(), F_OP) as i32;
    let state = field(h(), F_GEN_STATE);
    if state == GEN_RUNNING {
        type_error("Generator is already running");
    }
    if state == GEN_DONE {
        return done();
    }
    if state == GEN_START {
        set_field(h(), F_GEN_STATE, GEN_DONE);
        // concat has opened nothing yet.
        if op != OP_CONCAT {
            close_normal(field(h(), F_ITERATED));
        }
        return done();
    }
    // Suspended at a yield: resume with a return completion, which closes
    // flatMap's inner iterator first, then the underlying one (concat has
    // only its active inner iterator).
    set_field(h(), F_GEN_STATE, GEN_RUNNING);
    let result = crate::exception::catch_js_throw(|| {
        let inner = field(h(), F_STATE);
        if op == OP_CONCAT {
            if is_object(inner) {
                close_normal(inner);
            }
            return;
        }
        if op == OP_FLATMAP && is_object(inner) {
            if let Err(error) = crate::exception::catch_js_throw(|| close_normal(inner)) {
                let scope = crate::gc::RuntimeHandleScope::new();
                let error = scope.root_nanbox_f64(error);
                close_quietly(field(h(), F_ITERATED));
                crate::exception::js_throw(error.get_nanbox_f64());
            }
        }
        close_normal(field(h(), F_ITERATED));
    });
    set_field(h(), F_GEN_STATE, GEN_DONE);
    if let Err(error) = result {
        crate::exception::js_throw(error);
    }
    done()
}

/// Drain a record or helper into a new array (NaN-boxed).
pub(crate) unsafe fn drain_to_array(iterated: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iterated = scope.root_nanbox_f64(iterated);
    let arr = scope.root_raw_mut_ptr(crate::array::js_array_alloc(8));
    while let Some(v) = helper_step(iterated.get_nanbox_f64()) {
        arr.set_raw_mut_ptr(crate::array::js_array_push_f64(arr.get_raw_mut_ptr(), v));
    }
    js_nanbox_pointer(arr.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64)
}
