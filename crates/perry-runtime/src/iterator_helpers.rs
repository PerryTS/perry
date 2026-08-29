//! Global `Iterator` helper objects (TC39 Iterator-helpers proposal, Node 22+;
//! issue #2874).
//!
//! Node exposes a global `Iterator` (typeof `"function"`) with a static
//! `Iterator.from(x)` and lazy helper methods on the iterator prototype:
//! `.map`, `.filter`, `.take`, `.drop`, `.flatMap`, `.toArray`, `.forEach`,
//! `.reduce`, `.some`, `.every`, `.find`. They operate LAZILY on any
//! synchronous iterator — generators, the Map/Set/array iterator objects Perry
//! already builds, or a bare `{ next() }` object.
//!
//! Representation mirrors `collection_iter_object.rs`: a regular `ObjectHeader`
//! with a dedicated class id (`ITERATOR_HELPER_CLASS_ID`). Fields:
//!   - field 0: source iterator (NaN-boxed; kept alive by the object scanner)
//!   - field 1: op kind (number; see `OP_*` below)
//!   - field 2: callback closure (NaN-boxed) for map/filter/flatMap, or the
//!     numeric limit/count for take/drop
//!   - field 3: mutable state — remaining count for take, drop-counter for
//!     drop, an inner sub-iterator for flatMap (NaN-boxed pointer or 0).
//!
//! Each `.next()` pulls lazily from the source via [`iterator_step`], which
//! drives the generic `.next()` protocol on ANY iterator object (stored-closure
//! `next` field OR class-id method dispatch). Lazy chaining means `.take(2)` on
//! an infinite generator terminates. Terminal methods (`toArray`/`forEach`/
//! `reduce`/`some`/`every`/`find`) drain the chain.
//!
//! Dispatch lives in `object/native_call_method.rs` via the class-id check next
//! to the Map/Set iterator ones.

use crate::closure::{is_closure_ptr, js_closure_call1, js_closure_call2, ClosureHeader};
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
use crate::string::js_string_from_bytes;
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_TRUE, TAG_UNDEFINED};

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

// Op kinds stored in field 1.
const OP_IDENTITY: i32 = 0; // `Iterator.from(x)` — yields the source unchanged.
const OP_MAP: i32 = 1;
const OP_FILTER: i32 = 2;
const OP_TAKE: i32 = 3;
const OP_DROP: i32 = 4;
const OP_FLATMAP: i32 = 5;

pub fn is_iterator_helper_addr(addr: usize) -> bool {
    if addr < 0x1000 || (addr as u64) >> 48 != 0 {
        return false;
    }
    unsafe {
        let gc_header =
            (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return false;
        }
        (*(addr as *const ObjectHeader)).class_id == ITERATOR_HELPER_CLASS_ID
    }
}

/// Drive one `.next()` step on ANY iterator object. Returns `(value, done)`.
/// Mirrors the dual dispatch in `array/iterator.rs::js_iterator_to_array`:
/// prefer an OWN `next` closure FIELD (generators / bare `{next}` objects),
/// else fall back to class-id method dispatch (array / Map / Set / helper
/// iterators).
///
/// #7576: the own-vs-inherited distinction is the whole ballgame, and reading
/// it with the INHERITING getter is what killed the entire helper surface.
/// Every built-in iterator now inherits `.next` from its shared
/// `%…IteratorPrototype%` singleton (`object/iterator_prototypes.rs`), and that
/// inherited `next` is a THUNK that resolves its receiver from
/// `js_implicit_this_get()` and dispatches by class id. So
/// `js_object_get_field_by_name` found a perfectly callable closure for an
/// array / Map / Set / String iterator, this function took the closure-call
/// branch, and the thunk ran with whatever `this` happened to be left in the
/// thread-local — reporting `done` on the first step for every source kind, or
/// throwing `Method %IteratorPrototype%.next called on incompatible receiver`.
/// `js_iterator_to_array` already carries the own-field version of this fix
/// (#321); this is the same fix for the helper chain.
///
/// The call itself binds `this` to the iterator per `IteratorNext`'s
/// `Call(next, iterator)`. That matters for the own-field case too: a `next()`
/// method written as `next() { return this.#impl.next(); }` gets the receiver
/// it is entitled to instead of `undefined`.
///
/// #7564: the READ side of the protocol had the same defect as the WRITE side.
/// It allocated the constant key strings on EVERY step, and held `iter_obj`,
/// `result_obj` and the earlier keys in bare Rust locals across those
/// allocations — plus, in the middle, a call into the user's own `next`, which
/// can run arbitrary JS and collect anything. The keys are now interned
/// (allocation-free after the first call per thread) or read by byte slice
/// (`js_object_get_own_field_or_undef`, which allocates nothing at all), and
/// every pointer is re-read from a handle after each call that can collect, so
/// no pre-collection address is nameable.
unsafe fn iterator_step(iter_f64: f64) -> (f64, bool) {
    if js_nanbox_get_pointer(iter_f64) == 0 {
        return (f64::from_bits(TAG_UNDEFINED), true);
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let iter_h = scope.root_nanbox_f64(iter_f64);

    // OWN field only — see the `#7576` note above. Allocation-free (it matches
    // the key by byte slice), so no `across_*` is needed to reach it.
    let next_val = JSValue::from_bits(
        crate::object::js_object_get_own_field_or_undef(
            iter_h.get_nanbox_f64(),
            b"next".as_ptr(),
            4,
        )
        .to_bits(),
    );
    let next_ptr = if next_val.is_undefined() {
        std::ptr::null::<ClosureHeader>()
    } else {
        js_nanbox_get_pointer(f64::from_bits(next_val.bits())) as *const ClosureHeader
    };
    let use_field = !next_ptr.is_null() && is_closure_ptr(next_ptr as usize);

    // The user's `next` runs here and can collect anything, so the receiver and
    // the saved implicit-`this` both come out of handles rather than out of
    // bare locals that predate the call.
    let result_f64 = if use_field {
        let next_h = scope.root_nanbox_f64(f64::from_bits(next_val.bits()));
        let prev_this_h =
            scope.root_nanbox_f64(crate::object::js_implicit_this_set(iter_h.get_nanbox_f64()));
        let r = crate::closure::js_native_call_value(next_h.get_nanbox_f64(), std::ptr::null(), 0);
        crate::object::js_implicit_this_set(prev_this_h.get_nanbox_f64());
        r
    } else {
        crate::object::js_native_call_method(
            iter_h.get_nanbox_f64(),
            b"next".as_ptr() as *const i8,
            4,
            std::ptr::null(),
            0,
        )
    };

    if js_nanbox_get_pointer(result_f64) == 0 {
        return (f64::from_bits(TAG_UNDEFINED), true);
    }
    // Both reads below re-take the result's address across their own intern,
    // and `js_object_get_field_by_name` (getters, shape work) can collect
    // between them — so the second read does not reuse the first's address.
    let result_h = scope.root_nanbox_f64(result_f64);
    let (done_key, result_now) =
        result_h.across_nanbox(|| crate::string::intern_ascii_literal(b"done"));
    let done_val = js_object_get_field_by_name(
        js_nanbox_get_pointer(result_now) as *const ObjectHeader,
        done_key,
    );
    let done = crate::value::js_is_truthy(f64::from_bits(done_val.bits())) != 0;
    let (value_key, result_now) =
        result_h.across_nanbox(|| crate::string::intern_ascii_literal(b"value"));
    let val = js_object_get_field_by_name(
        js_nanbox_get_pointer(result_now) as *const ObjectHeader,
        value_key,
    );
    (f64::from_bits(val.bits()), done)
}

/// Get the iterator of an iterable value (array / generator / Map/Set iterator
/// object / anything with `[Symbol.iterator]` or a `.next`). Reuses
/// `js_get_iterator`, which already returns the value unchanged when it is
/// itself an iterator object.
unsafe fn get_iterator(val_f64: f64) -> f64 {
    crate::symbol::js_get_iterator(val_f64)
}

/// Extract a closure pointer from a NaN-boxed value, or null.
unsafe fn closure_ptr(val_f64: f64) -> *const ClosureHeader {
    let p = js_nanbox_get_pointer(val_f64) as *const ClosureHeader;
    if !p.is_null() && is_closure_ptr(p as usize) {
        p
    } else {
        std::ptr::null()
    }
}

unsafe fn alloc_helper(op: i32, source: f64, arg: f64) -> f64 {
    let obj = js_object_alloc(ITERATOR_HELPER_CLASS_ID, 4);
    js_object_set_field(obj, 0, JSValue::from_bits(source.to_bits()));
    js_object_set_field(obj, 1, JSValue::number(op as f64));
    js_object_set_field(obj, 2, JSValue::from_bits(arg.to_bits()));
    // Field 3: mutable state. take/drop seed it from the count; flatMap seeds
    // the inner sub-iterator slot to 0 (none yet).
    let state = match op {
        OP_TAKE | OP_DROP => arg,
        _ => f64::from_bits(TAG_UNDEFINED),
    };
    js_object_set_field(obj, 3, JSValue::from_bits(state.to_bits()));
    // Helper objects have their own `%Iterator Helper Prototype%`, whose
    // intrinsic `next` is readable as a function value. This also gives every
    // helper the shared `%IteratorPrototype%` parent used by the other builtin
    // iterator families.
    crate::object::attach_iterator_prototype(obj, ITERATOR_HELPER_CLASS_ID);
    js_nanbox_pointer(obj as i64)
}

/// `Iterator.from(x)` — wrap any iterable/iterator in an identity helper so the
/// helper methods are available. Returns a NaN-boxed pointer.
#[no_mangle]
pub extern "C" fn js_iterator_from(val_f64: f64) -> f64 {
    unsafe {
        let source = get_iterator(val_f64);
        // If the source is already one of our helper objects, hand it back
        // unchanged (Node returns the iterator itself when it already inherits
        // from Iterator.prototype).
        let p = js_nanbox_get_pointer(source);
        if p != 0 && is_iterator_helper_addr(p as usize) {
            return source;
        }
        alloc_helper(OP_IDENTITY, source, f64::from_bits(TAG_UNDEFINED))
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ITERATOR_FROM: extern "C" fn(f64) -> f64 = js_iterator_from;

/// `.next()` on a helper iterator object. Pulls lazily from the source per the
/// op kind.
unsafe fn helper_next(obj: *mut ObjectHeader) -> f64 {
    let source = f64::from_bits(js_object_get_field(obj, 0).bits());
    let op = f64::from_bits(js_object_get_field(obj, 1).bits()) as i32;
    let arg = f64::from_bits(js_object_get_field(obj, 2).bits());

    match op {
        OP_IDENTITY => {
            let (v, done) = iterator_step(source);
            make_iter_result(JSValue::from_bits(v.to_bits()), done)
        }
        OP_MAP => {
            let cb = closure_ptr(arg);
            let (v, done) = iterator_step(source);
            if done {
                return make_iter_result(JSValue::undefined(), true);
            }
            let mapped = if cb.is_null() {
                v
            } else {
                js_closure_call1(cb, v)
            };
            make_iter_result(JSValue::from_bits(mapped.to_bits()), false)
        }
        OP_FILTER => {
            let cb = closure_ptr(arg);
            loop {
                let (v, done) = iterator_step(source);
                if done {
                    return make_iter_result(JSValue::undefined(), true);
                }
                let keep = if cb.is_null() {
                    true
                } else {
                    crate::value::js_is_truthy(js_closure_call1(cb, v)) != 0
                };
                if keep {
                    return make_iter_result(JSValue::from_bits(v.to_bits()), false);
                }
            }
        }
        OP_TAKE => {
            let remaining = f64::from_bits(js_object_get_field(obj, 3).bits());
            if !(remaining > 0.0) {
                return make_iter_result(JSValue::undefined(), true);
            }
            js_object_set_field(obj, 3, JSValue::number(remaining - 1.0));
            let (v, done) = iterator_step(source);
            if done {
                return make_iter_result(JSValue::undefined(), true);
            }
            make_iter_result(JSValue::from_bits(v.to_bits()), false)
        }
        OP_DROP => {
            // Drop the first N (once), then passthrough.
            let mut to_drop = f64::from_bits(js_object_get_field(obj, 3).bits());
            while to_drop > 0.0 {
                let (_v, done) = iterator_step(source);
                if done {
                    js_object_set_field(obj, 3, JSValue::number(0.0));
                    return make_iter_result(JSValue::undefined(), true);
                }
                to_drop -= 1.0;
            }
            js_object_set_field(obj, 3, JSValue::number(0.0));
            let (v, done) = iterator_step(source);
            make_iter_result(JSValue::from_bits(v.to_bits()), done)
        }
        OP_FLATMAP => {
            let cb = closure_ptr(arg);
            loop {
                // Drain the current inner sub-iterator first.
                let inner = f64::from_bits(js_object_get_field(obj, 3).bits());
                let inner_ptr = js_nanbox_get_pointer(inner);
                if inner_ptr != 0 {
                    let (v, done) = iterator_step(inner);
                    if !done {
                        return make_iter_result(JSValue::from_bits(v.to_bits()), false);
                    }
                    // Inner exhausted — clear and pull the next outer value.
                    js_object_set_field(obj, 3, JSValue::from_bits(TAG_UNDEFINED));
                }
                let (v, done) = iterator_step(source);
                if done {
                    return make_iter_result(JSValue::undefined(), true);
                }
                let produced = if cb.is_null() {
                    v
                } else {
                    js_closure_call1(cb, v)
                };
                // Per spec each produced value must itself be iterable; wrap it.
                let inner_iter = get_iterator(produced);
                js_object_set_field(obj, 3, JSValue::from_bits(inner_iter.to_bits()));
            }
        }
        _ => make_iter_result(JSValue::undefined(), true),
    }
}

/// Run the intrinsic `Iterator Helper.prototype.next` algorithm without
/// consulting instance or prototype overrides. The canonical prototype thunk
/// uses this entry point so a saved `helper.next.bind(helper)` keeps advancing
/// the helper it captured even if user code later replaces a `next` property.
pub(crate) unsafe fn iterator_helper_next_builtin(obj: *mut ObjectHeader) -> f64 {
    helper_next(obj)
}

/// Drain the helper iterator fully into a `*mut ArrayHeader` (NaN-box yourself).
unsafe fn helper_to_array(obj: *mut ObjectHeader) -> f64 {
    let mut arr = crate::array::js_array_alloc(8);
    for _ in 0..100_000_000usize {
        let res = helper_next(obj);
        let res_ptr = js_nanbox_get_pointer(res);
        if res_ptr == 0 {
            break;
        }
        let done = crate::value::js_is_truthy(f64::from_bits(
            js_object_get_field(res_ptr as *mut ObjectHeader, 1).bits(),
        )) != 0;
        if done {
            break;
        }
        let v = f64::from_bits(js_object_get_field(res_ptr as *mut ObjectHeader, 0).bits());
        arr = crate::array::js_array_push_f64(arr, v);
    }
    js_nanbox_pointer(arr as i64)
}

/// Is `name` one of the iterator-helper method names?
pub fn is_iterator_helper_method(name: &str) -> bool {
    matches!(
        name,
        "map"
            | "filter"
            | "take"
            | "drop"
            | "flatMap"
            | "toArray"
            | "forEach"
            | "reduce"
            | "some"
            | "every"
            | "find"
    )
}

/// #2874: when a method call lands on a RAW iterator object (a generator, a
/// Map/Set/array iterator, or any `{ next() }`) for an iterator-helper method
/// it doesn't define as an own property, Node resolves it on
/// `Iterator.prototype`. Wrap the iterator in an identity helper and dispatch
/// there. Returns `Some(result)` when handled, `None` to fall through.
///
/// `has_own` reports whether `obj` defines `method_name` as an own callable
/// field (in which case the user's own method wins and we must NOT intercept).
pub unsafe fn maybe_dispatch_helper_on_iterator(
    obj: *mut ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
    has_own_field: bool,
) -> Option<f64> {
    if has_own_field || !is_iterator_helper_method(method_name) {
        return None;
    }
    // Only intercept genuine iterators: those exposing a callable `.next`.
    let next_key = js_string_from_bytes(b"next".as_ptr(), 4);
    let next_val = js_object_get_field_by_name(obj, next_key);
    if next_val.is_undefined() {
        return None;
    }
    let next_ptr = js_nanbox_get_pointer(f64::from_bits(next_val.bits())) as *const ClosureHeader;
    if next_ptr.is_null() || !is_closure_ptr(next_ptr as usize) {
        return None;
    }
    let self_f64 = js_nanbox_pointer(obj as i64);
    let wrapped = js_iterator_from(self_f64);
    let wrapped_ptr = js_nanbox_get_pointer(wrapped) as *mut ObjectHeader;
    Some(dispatch_iterator_helper_method(
        wrapped_ptr,
        method_name,
        args_ptr,
        args_len,
    ))
}

/// Dispatch an iterator-helper method on a runtime builtin iterator object.
/// The native iterator class-id arms normally handle protocol methods such as
/// `next` directly; helper names must instead wrap that iterator and enter the
/// lazy helper dispatcher. Callers have already verified the builtin iterator
/// class id before reaching this function.
pub unsafe fn maybe_dispatch_helper_on_builtin_iterator(
    obj: *mut ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !is_iterator_helper_method(method_name) {
        return None;
    }
    let source = js_nanbox_pointer(obj as i64);
    let wrapped = js_iterator_from(source);
    let wrapped_ptr = js_nanbox_get_pointer(wrapped) as *mut ObjectHeader;
    Some(dispatch_iterator_helper_method(
        wrapped_ptr,
        method_name,
        args_ptr,
        args_len,
    ))
}

/// Dispatch a method call on a helper iterator object. `args_ptr`/`args_len`
/// carry the NaN-boxed call arguments.
pub unsafe fn dispatch_iterator_helper_method(
    obj: *mut ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let self_f64 = js_nanbox_pointer(obj as i64);
    let arg0 = if args_len >= 1 && !args_ptr.is_null() {
        *args_ptr
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let arg1 = if args_len >= 2 && !args_ptr.is_null() {
        *args_ptr.add(1)
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };

    match method_name {
        // #9019: an own `next` assigned onto the helper instance wins over
        // the builtin advance, exactly as on the Map/Set path.
        "next" => {
            if let Some(result) =
                crate::object::call_overridden_iterator_next(obj, ITERATOR_HELPER_CLASS_ID)
            {
                return result;
            }
            helper_next(obj)
        }
        "Symbol.iterator" | "@@iterator" => self_f64,
        "return" | "throw" => make_iter_result(JSValue::undefined(), true),
        // Lazy helpers — return a new helper wrapping `self`.
        "map" => alloc_helper(OP_MAP, self_f64, arg0),
        "filter" => alloc_helper(OP_FILTER, self_f64, arg0),
        "flatMap" => alloc_helper(OP_FLATMAP, self_f64, arg0),
        "take" => {
            let n = JSValue::from_bits(arg0.to_bits()).to_number();
            let count = if n.is_nan() { 0.0 } else { n.max(0.0).floor() };
            alloc_helper(
                OP_TAKE,
                self_f64,
                f64::from_bits(JSValue::number(count).bits()),
            )
        }
        "drop" => {
            let n = JSValue::from_bits(arg0.to_bits()).to_number();
            let count = if n.is_nan() { 0.0 } else { n.max(0.0).floor() };
            alloc_helper(
                OP_DROP,
                self_f64,
                f64::from_bits(JSValue::number(count).bits()),
            )
        }
        // Terminal helpers — drain.
        "toArray" => helper_to_array(obj),
        "forEach" => {
            let cb = closure_ptr(arg0);
            let mut i = 0.0f64;
            loop {
                let (v, done) = iterator_step(self_f64);
                if done {
                    break;
                }
                if !cb.is_null() {
                    js_closure_call2(cb, v, f64::from_bits(JSValue::number(i).bits()));
                }
                i += 1.0;
            }
            f64::from_bits(TAG_UNDEFINED)
        }
        "reduce" => {
            let cb = closure_ptr(arg0);
            let has_init = args_len >= 2;
            let mut acc = arg1;
            let mut started = has_init;
            loop {
                let (v, done) = iterator_step(self_f64);
                if done {
                    break;
                }
                if !started {
                    acc = v;
                    started = true;
                    continue;
                }
                if !cb.is_null() {
                    acc = js_closure_call2(cb, acc, v);
                }
            }
            if !started {
                // No init + empty iterator → TypeError in spec; return undefined
                // (Perry avoids throwing from this internal helper).
                return f64::from_bits(TAG_UNDEFINED);
            }
            acc
        }
        "some" => {
            let cb = closure_ptr(arg0);
            loop {
                let (v, done) = iterator_step(self_f64);
                if done {
                    return f64::from_bits(crate::value::TAG_FALSE);
                }
                if !cb.is_null() && crate::value::js_is_truthy(js_closure_call1(cb, v)) != 0 {
                    return f64::from_bits(TAG_TRUE);
                }
            }
        }
        "every" => {
            let cb = closure_ptr(arg0);
            loop {
                let (v, done) = iterator_step(self_f64);
                if done {
                    return f64::from_bits(TAG_TRUE);
                }
                if !cb.is_null() && crate::value::js_is_truthy(js_closure_call1(cb, v)) == 0 {
                    return f64::from_bits(crate::value::TAG_FALSE);
                }
            }
        }
        "find" => {
            let cb = closure_ptr(arg0);
            loop {
                let (v, done) = iterator_step(self_f64);
                if done {
                    return f64::from_bits(TAG_UNDEFINED);
                }
                if !cb.is_null() && crate::value::js_is_truthy(js_closure_call1(cb, v)) != 0 {
                    return f64::from_bits(v.to_bits());
                }
            }
        }
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
