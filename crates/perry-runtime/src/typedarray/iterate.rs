//! `%TypedArray%.prototype` iteration methods (map/filter/every/some/forEach/
//! find/findIndex/reduce/reduceRight). Split out of `typedarray/mod.rs`.

use super::*;

use crate::closure::ClosureHeader;

// %TypedArray%.prototype iteration methods. The generic `js_array_*` helpers
// detect a TypedArray receiver via `lookup_typed_array_kind` and delegate
// here (mirroring the existing sort / at / findLast delegation), so these
// read elements through the element-typed `load_at` instead of reinterpreting
// the raw int/float storage as NaN-boxed f64 (which produced garbage values).
// The callback receives `(element, index)` — same 2-arg convention the rest of
// this file and the generic array helpers use.

/// The GC custody every callback loop in this file needs, held in ONE place.
///
/// A loop here runs user code once per element, and a user callback can
/// collect. Three values cross those calls: the receiver, the callback closure
/// and — for `map` — the freshly created result. Each used to live in a bare
/// Rust local, on the argument that `typed_array_alloc` places TypedArrays in
/// the old arena as TENURED and so they do not MOVE. That argument is about
/// relocation only. A full mark-sweep still RECLAIMS an old-arena object that
/// nothing marks, and a closure both moves and dies: `map`'s result has no
/// referent at all until it is returned, and on the reflective entry
/// (`Uint8Array.prototype.map.call(u, cb)`) the callback is reachable only
/// through the thunk's argument array on the native stack. A full collection
/// during a callback freed `map`'s result and the caller received a swept cell
/// (`test_gap_gc_uint8_buffer_callback_rooting`: `protoMap 1`, on every arm
/// that runs the non-generational full collector).
///
/// So the receiver is rooted here for liveness (its raw address stays valid:
/// it does not move), the callback is rooted AND re-read before every call
/// (it does move), and [`CallbackLoop::keep`] roots a result for the rest of
/// the loop. A helper cannot obtain a callback to call except through this
/// type.
pub(super) struct CallbackLoop<'s> {
    scope: &'s crate::gc::RuntimeHandleScope,
    recv: f64,
    /// The callback's `this` (the `thisArg` argument), rooted: it can be a
    /// movable heap object and the callback can collect.
    this_arg: crate::gc::RuntimeHandle<'s>,
    /// `None` only under the `typed_loop_root` sabotage, which leaves every
    /// value of the loop unrooted so its witness can tell the difference.
    callback: Option<crate::gc::RuntimeHandle<'s>>,
    unrooted_callback: *const ClosureHeader,
}

#[cfg(test)]
fn loop_roots_sabotaged() -> bool {
    crate::buffer::bytes::b4_sabotage("typed_loop_root")
}

#[cfg(not(test))]
#[inline(always)]
fn loop_roots_sabotaged() -> bool {
    false
}

impl<'s> CallbackLoop<'s> {
    pub(super) fn new(
        scope: &'s crate::gc::RuntimeHandleScope,
        ta: *const TypedArrayHeader,
        callback: *const ClosureHeader,
    ) -> Self {
        Self::with_this(
            scope,
            ta,
            callback,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        )
    }

    pub(super) fn with_this(
        scope: &'s crate::gc::RuntimeHandleScope,
        ta: *const TypedArrayHeader,
        callback: *const ClosureHeader,
        this_arg: f64,
    ) -> Self {
        let rooted = !loop_roots_sabotaged();
        if rooted {
            scope.root_raw_const_ptr(ta);
        }
        Self {
            scope,
            recv: ta_receiver_value(ta),
            this_arg: scope.root_nanbox_f64(this_arg),
            callback: rooted.then(|| scope.root_raw_const_ptr(callback)),
            unrooted_callback: callback,
        }
    }

    fn current(&self) -> *const ClosureHeader {
        self.callback
            .as_ref()
            .map_or(self.unrooted_callback, |root| root.get_raw_const_ptr())
    }

    /// Root `value` until the loop ends.
    pub(super) fn keep(&self, value: f64) {
        if self.callback.is_some() {
            self.scope.root_nanbox_f64(value);
        }
    }

    /// `cb(v, i, receiver)` with the callback's CURRENT address.
    pub(super) fn call3(&self, site: &crate::closure::DirectCall3, v: f64, i: usize) -> f64 {
        site.call(
            self.current(),
            crate::closure::JsThis::from_f64(self.this_arg.get_nanbox_f64()),
            v,
            i as f64,
            self.recv,
        )
    }

    /// `cb(acc, v, i, receiver)` with the callback's CURRENT address.
    pub(super) fn call4(
        &self,
        site: &crate::closure::DirectCall4,
        acc: f64,
        v: f64,
        i: usize,
    ) -> f64 {
        site.call(
            self.current(),
            crate::closure::JsThis::from_f64(self.this_arg.get_nanbox_f64()),
            acc,
            v,
            i as f64,
            self.recv,
        )
    }
}

/// `ta.map(cb)` — returns a NEW TypedArray of the SAME kind (per spec, not a
/// plain Array). Each result is coerced back to the element type via the same
/// `jsvalue_to_f64` path `ta[i] = v` uses.
#[no_mangle]
pub extern "C" fn js_typed_array_map(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    typed_array_map_this(ta, callback, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `ta.map(cb, thisArg)`: `this_arg` is the callback's `this`.
pub(crate) fn typed_array_map_this(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = crate::typedarray::element_kind(ta);
        let len = crate::typedarray::element_length(ta) as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let calls = CallbackLoop::with_this(&scope, ta, callback, this_arg);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        // 23.2.3.20 step 5: A is TypedArraySpeciesCreate(O, « len ») — BEFORE
        // the callback loop (so a throwing constructor/@@species getter aborts
        // before any callback runs).
        let choice = species::species_constructor(ta as usize, kind);
        let result = species::species_create_length(&choice, kind, len);
        let Some(result_addr) = crate::typedarray_props::typed_array_addr_from_value(result) else {
            return species::result_as_ptr(result);
        };
        // The result is referenced by nothing but this frame until it returns.
        calls.keep(result);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = calls.call3(&cb_site, v, i);
            crate::typedarray_props::species_result_store(result_addr, i, r);
        }
        species::result_as_ptr(result)
    }
}

/// `ta.filter(cb)` — returns a NEW TypedArray of the SAME kind holding the
/// elements for which `cb` returned truthy.
#[no_mangle]
pub extern "C" fn js_typed_array_filter(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    typed_array_filter_this(ta, callback, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `ta.filter(cb, thisArg)`: `this_arg` is the callback's `this`.
pub(crate) fn typed_array_filter_this(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = crate::typedarray::element_kind(ta);
        let len = crate::typedarray::element_length(ta) as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let calls = CallbackLoop::with_this(&scope, ta, callback, this_arg);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        // 23.2.3.10: the callback runs for every element FIRST (collecting the
        // kept values), THEN A = TypedArraySpeciesCreate(O, « captured »). The
        // @@species getter is therefore observed after all callbacks.
        let mut kept: Vec<f64> = Vec::new();
        for i in 0..len {
            let v = load_at(ta, i);
            let r = calls.call3(&cb_site, v, i);
            if crate::value::js_is_truthy(r) != 0 {
                kept.push(v);
            }
        }
        let choice = species::species_constructor(ta as usize, kind);
        let result = species::species_create_length(&choice, kind, kept.len());
        let Some(result_addr) = crate::typedarray_props::typed_array_addr_from_value(result) else {
            return species::result_as_ptr(result);
        };
        for (i, v) in kept.into_iter().enumerate() {
            crate::typedarray_props::species_result_store(result_addr, i, v);
        }
        species::result_as_ptr(result)
    }
}

/// `ta.every(cb)` — NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_every(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    typed_array_every_this(ta, callback, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `ta.every(cb, thisArg)`: `this_arg` is the callback's `this`.
pub(crate) fn typed_array_every_this(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_TRUE);
    }
    unsafe {
        let len = crate::typedarray::element_length(ta) as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let calls = CallbackLoop::with_this(&scope, ta, callback, this_arg);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = calls.call3(&cb_site, v, i);
            if crate::value::js_is_truthy(r) == 0 {
                return f64::from_bits(crate::value::TAG_FALSE);
            }
        }
        f64::from_bits(crate::value::TAG_TRUE)
    }
}

/// `ta.some(cb)` — NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_some(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    typed_array_some_this(ta, callback, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `ta.some(cb, thisArg)`: `this_arg` is the callback's `this`.
pub(crate) fn typed_array_some_this(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    unsafe {
        let len = crate::typedarray::element_length(ta) as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let calls = CallbackLoop::with_this(&scope, ta, callback, this_arg);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = calls.call3(&cb_site, v, i);
            if crate::value::js_is_truthy(r) != 0 {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
        }
        f64::from_bits(crate::value::TAG_FALSE)
    }
}

/// `ta.forEach(cb)` — returns undefined.
#[no_mangle]
pub extern "C" fn js_typed_array_for_each(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    typed_array_for_each_this(ta, callback, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `ta.for_each(cb, thisArg)`: `this_arg` is the callback's `this`.
pub(crate) fn typed_array_for_each_this(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if !ta.is_null() {
        unsafe {
            let len = crate::typedarray::element_length(ta) as usize;
            let scope = crate::gc::RuntimeHandleScope::new();
            let calls = CallbackLoop::with_this(&scope, ta, callback, this_arg);
            // #8180: resolve the callback's dispatch ONCE. It is invariant for a
            // fixed closure (see closure/dispatch/direct.rs), and this loop calls
            // exactly one.
            let cb_site = crate::closure::DirectCall3::resolve(callback);
            for i in 0..len {
                let v = load_at(ta, i);
                let _ = calls.call3(&cb_site, v, i);
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `ta.find(cb)` — first element for which `cb` is truthy, else undefined.
#[no_mangle]
pub extern "C" fn js_typed_array_find(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    typed_array_find_this(ta, callback, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `ta.find(cb, thisArg)`: `this_arg` is the callback's `this`.
pub(crate) fn typed_array_find_this(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let len = crate::typedarray::element_length(ta) as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let calls = CallbackLoop::with_this(&scope, ta, callback, this_arg);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = calls.call3(&cb_site, v, i);
            if crate::value::js_is_truthy(r) != 0 {
                return v;
            }
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// `ta.findIndex(cb)` — first matching index as plain f64, else -1.
#[no_mangle]
pub extern "C" fn js_typed_array_find_index(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    typed_array_find_index_this(ta, callback, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `ta.find_index(cb, thisArg)`: `this_arg` is the callback's `this`.
pub(crate) fn typed_array_find_index_this(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return -1.0;
    }
    unsafe {
        let len = crate::typedarray::element_length(ta) as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let calls = CallbackLoop::with_this(&scope, ta, callback, this_arg);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = calls.call3(&cb_site, v, i);
            if crate::value::js_is_truthy(r) != 0 {
                return i as f64;
            }
        }
        -1.0
    }
}

/// `ta.reduce(cb, initial?)` — accumulate left→right. Reads elements through
/// `load_at` (element-typed) and calls the reducer as
/// `(accumulator, currentValue, currentIndex, array)`. Throws
/// `TypeError: Reduce of empty array with no initial value` when the typed
/// array is empty and no initial value was provided. Issue #2799.
#[no_mangle]
pub extern "C" fn js_typed_array_reduce(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    // #11419: the callback is a plain call, so it runs with `this` undefined,
    // not the receiver of whatever method dispatch encloses this call.
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        if has_initial != 0 {
            return initial;
        }
        crate::array::throw_reduce_of_empty();
    }
    unsafe {
        let len = crate::typedarray::element_length(ta) as usize;
        if len == 0 {
            if has_initial != 0 {
                return initial;
            }
            crate::array::throw_reduce_of_empty();
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let calls = CallbackLoop::new(&scope, ta, callback);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall4::resolve(callback);
        let (accumulator, start_idx) = if has_initial != 0 {
            (initial, 0)
        } else {
            (load_at(ta, 0), 1)
        };
        // #8179: root the accumulator. It is the one value in these loops
        // that is routinely a NURSERY object — a string/object/array seed, or
        // a fresh one returned by every callback — and a bare Rust local
        // holding a NaN-boxed pre-move address is a copy the collector cannot
        // see. `js_array_reduce` (array/iter_methods.rs) has rooted its
        // accumulator since the 2026-07-02 audit; the TypedArray siblings and
        // the Buffer-receiver dispatcher never did.
        let acc = scope.root_nanbox_f64(accumulator);
        for i in start_idx..len {
            let v = load_at(ta, i);
            let next = calls.call4(&cb_site, acc.get_nanbox_f64(), v, i);
            acc.set_nanbox_f64(next);
        }
        acc.get_nanbox_f64()
    }
}

/// `ta.reduceRight(cb, initial?)` — accumulate right→left. Same reducer
/// contract as `js_typed_array_reduce`. Issue #2799.
#[no_mangle]
pub extern "C" fn js_typed_array_reduce_right(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    // #11419: the callback is a plain call, so it runs with `this` undefined,
    // not the receiver of whatever method dispatch encloses this call.
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        if has_initial != 0 {
            return initial;
        }
        crate::array::throw_reduce_of_empty();
    }
    unsafe {
        let len = crate::typedarray::element_length(ta) as usize;
        if len == 0 {
            if has_initial != 0 {
                return initial;
            }
            crate::array::throw_reduce_of_empty();
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let calls = CallbackLoop::new(&scope, ta, callback);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall4::resolve(callback);
        let (accumulator, start_idx) = if has_initial != 0 {
            (initial, len)
        } else {
            (load_at(ta, len - 1), len - 1)
        };
        // #8179: root the accumulator. It is the one value in these loops
        // that is routinely a NURSERY object — a string/object/array seed, or
        // a fresh one returned by every callback — and a bare Rust local
        // holding a NaN-boxed pre-move address is a copy the collector cannot
        // see. `js_array_reduce` (array/iter_methods.rs) has rooted its
        // accumulator since the 2026-07-02 audit; the TypedArray siblings and
        // the Buffer-receiver dispatcher never did.
        let acc = scope.root_nanbox_f64(accumulator);
        if start_idx > 0 {
            for i in (0..start_idx).rev() {
                let v = load_at(ta, i);
                let next = calls.call4(&cb_site, acc.get_nanbox_f64(), v, i);
                acc.set_nanbox_f64(next);
            }
        }
        acc.get_nanbox_f64()
    }
}
