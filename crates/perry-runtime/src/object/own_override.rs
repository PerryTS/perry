//! "Could this receiver own a property that shadows the builtin?" (#10943)
//!
//! # The bug this answers
//!
//! ECMA-262 resolves `recv.m(a)` as `Get(recv, "m")` then `Call`, so an own
//! `m` beats a builtin. perry lowers a method call to a DIRECT native call
//! (`js_map_get`, `js_date_*`, `js_array_*`, …) whenever it can prove the
//! receiver's KIND — and an own property that shadows the method leaves that
//! proof completely intact: `const m = new Map(); m.get = () => 1` is still
//! provably a Map. The result was a silent, PLAUSIBLE wrong value: a
//! zero-argument `Map.prototype.get` returns `undefined`, `RegExp.test` a
//! boolean, `Date.getTime` a number. Nothing throws.
//!
//! #10476 fixed the same bug for UNPROVEN receivers — their runtime kind
//! picks the builtin or the universal dispatcher, "which finds an own or
//! inherited user method". **A proof of KIND was being treated as a proof of
//! NO OWN OVERRIDE.** This module supplies the missing half of the condition.
//!
//! # Shape of the answer
//!
//! [`js_receiver_may_own_named_method`] is the condition of a diamond the
//! caller emits: false takes the direct builtin call, true takes the
//! universal method dispatcher. It only ever CHOOSES A BRANCH. It does not
//! resolve the property and call it — an own slot can hold a builtin thunk
//! that dispatches by name again, and an earlier attempt at this fix did
//! exactly that and overflowed the stack on
//! `test_bound_timer_dispatch_roots_args_during_async_hook_init_gc`. A
//! fail-closed answer may decline a fast path; it may not substitute an
//! action of its own.
//!
//! # Why a global flag rather than a per-cell bit
//!
//! `GcHeader::_reserved` has **no free bits** (`gc/types.rs`'s bit map says so
//! outright, and bits 12/13 are actively ERASED by `set_layout_state` —
//! #8690 and #10842 both lost a flag there). Arrays already carry
//! `GC_ARRAY_NAMED_PROPS`, but Map/Set/Date/RegExp keep their own named
//! properties in a per-thread side table, so there is no per-cell bit to read
//! and their `ObjectMeta` is usually null — asking there would mean
//! MATERIALISING a record to read an almost-always-clear flag.
//!
//! So the first question is process-global: *has any non-`ObjectHeader` cell
//! anywhere ever taken a named property?* Overwhelmingly the answer is no and
//! the cost is one relaxed load. This is the `accessors_in_use` idiom the read
//! path already uses.
//!
//! The flag is **set-only and over-approximating**, both deliberately. Set-only
//! because clearing it on delete would reopen the hole for
//! delete-then-shadow, exactly as `GC_ARRAY_NAMED_PROPS` is monotonic. Over-
//! approximating because it is armed at the TOP of the exotic-store gauntlet,
//! before that gauntlet's per-kind branches (buffer table, stream table, the
//! meta/expando paths): there is no single install funnel, and a missed
//! installer is the same silent wrong value this module exists to remove.
//! Arming early covers every kind, including ones added later, and the only
//! cost of a spurious arm is that the diamond's slow side runs.

use std::sync::atomic::Ordering;

/// Has any non-`ObjectHeader` cell ever taken a named property?
///
/// Set-only. See the module docs for why it is armed early and never cleared.
/// Exported so EMITTED CODE can test it inline. The guard's common case is
/// "nothing anywhere has ever installed a named property on a non-ordinary
/// cell", and paying a call to learn that cost +38 instructions on every
/// proven-Map builtin call. This is the same shape as
/// `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`: a counter-shaped gate the
/// emitted guard reads with one monotonic load and a not-taken branch, with
/// the call left behind it for the case that is almost never taken.
///
/// A `u32` rather than a bool so the emitted load matches the barrier gate's
/// alignment and width; only zero / non-zero is meaningful.
#[no_mangle]
pub static PERRY_OWN_NAMED_PROP_INSTALLED: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// Armed from the top of `field_set_by_name`'s exotic-store gauntlet.
#[inline]
pub(crate) fn note_exotic_named_prop_install() {
    // Relaxed is enough: a stale `false` can only be read by a thread that has
    // not yet observed the store, and that thread's own receivers cannot be
    // the one just written (the write happens-before any publication of the
    // receiver to another thread — perry workers deep-copy rather than share
    // `ObjectHeader`s, #6185).
    if PERRY_OWN_NAMED_PROP_INSTALLED.load(Ordering::Relaxed) == 0 {
        PERRY_OWN_NAMED_PROP_INSTALLED.store(1, Ordering::Relaxed);
    }
}

/// May `recv` own a property named `name` that must beat a builtin?
///
/// `1` = maybe (take the universal dispatcher), `0` = provably not (take the
/// direct builtin call). **Never answers `0` for anything it cannot prove**,
/// which is the whole contract: a wrong `0` is a silent wrong value, a wrong
/// `1` is only slower.
///
/// # Safety
/// `name_ptr`/`name_len` must describe a live UTF-8 method name; `recv` is any
/// NaN-boxed value.
#[no_mangle]
pub unsafe extern "C" fn js_receiver_may_own_named_method(
    recv: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> i32 {
    let jsval = crate::JSValue::from_bits(recv.to_bits());
    if !jsval.is_pointer() {
        // A primitive's methods come from its wrapper prototype; it has no own
        // properties of its own.
        return 0;
    }
    let addr = (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if addr == 0 {
        return 0;
    }

    // An ARRAY records the fact on the cell, so it is answered exactly without
    // consulting the global arm at all: `GC_ARRAY_NAMED_PROPS` is set when an
    // array takes a named property and is monotonic for the same reason this
    // module's flag is.
    if let Some(header) = crate::value::addr_class::try_read_gc_header(addr) {
        if matches!(
            header.obj_type,
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
        ) {
            if header._reserved & crate::gc::GC_ARRAY_NAMED_PROPS != 0 {
                return 1;
            }
            // The bit is NOT a proof of absence, which this module's own rule
            // ("never answer 0 for anything it cannot prove") forbids relying
            // on. An array has a THIRD place for an own named property: the
            // descriptor side table that `get_accessor_descriptor` reads and
            // `array_own_key_present` consults BEFORE the bit-flagged named
            // props. `a.push = () => 1` lands there — measured: neither
            // `array_named_property_set` nor `expando_store` runs, yet
            // `hasOwn` is true, `typeof` is "function" and `Object.keys`
            // shows it. Reflection was right and this tier was reading the
            // wrong table.
            //
            // So when any descriptor exists anywhere, ask the authoritative
            // predicate instead of answering 0. When none does — the
            // overwhelmingly common case — the bit stands and this stays one
            // load and a test.
            // Nor is the descriptor side table the answer: with
            // `accessors_in_use` clear the row still ran the builtin, so the
            // property `a.push = () => 1` installs is in neither the
            // bit-flagged named props, the exotic expando, nor the accessor
            // descriptors — yet `hasOwn`/`typeof`/`Object.keys` all see it.
            //
            // MEASURED COST OF ASKING ANYWAY: +4,745 instructions per
            // guarded array builtin call (`a.push(i)` hot, 90.5 -> 4886.7 per
            // iteration; `a.indexOf` 701.0 -> 5442.6; an element-read control
            // flat at 16.05). The two deltas agree within 55 instructions on
            // calls whose own work differs by 610, so it is a fixed per-call
            // cost: the key-string allocation plus a full `hasOwn`. The exotic
            // kinds pay +38.000 for the same guard, because their flag lets
            // them answer 0.
            //
            // The cheap absence proof arrays were said to lack EXISTS, and
            // this tier simply was not wired to it.
            // `array_has_named_properties_resolved` covers all three storages
            // an array named property can live in — the inline reserve, the
            // pairs array, and the fallback table behind
            // `FULL_ARRAY_NAMED_PROPS_EVER` — and both spellings of an
            // own-method install go through `array_named_property_set`, which
            // writes one of them. That is profiled, not assumed: `a.push = fn`
            // on an `any` receiver and on a proven array local both show
            // `array::named_props::array_named_property_set` in the install
            // profile. (The earlier note here, that neither that function nor
            // the expando store runs, was measured on a path that no longer
            // carries this spelling and is wrong.)
            //
            // The descriptor table is still not covered by it, so a receiver
            // with ANY descriptor keeps asking the authoritative predicate —
            // the module's rule is unchanged, only the set of receivers that
            // can be proven absent has grown. With no descriptors,
            // `fallback_possible` is false, so the predicate is flag tests and
            // a reserve read: no hash lookup, no allocation, no call.
            if header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS == 0
                && header.obj_type == crate::gc::GC_TYPE_ARRAY
                && !crate::array::array_has_named_properties_resolved(
                    addr as *const crate::array::ArrayHeader,
                )
            {
                return 0;
            }
            return authoritative_has_own(recv, name_ptr, name_len);
        }
    } else {
        // No readable GC header. Cannot prove absence — decline.
        return 1;
    }

    if PERRY_OWN_NAMED_PROP_INSTALLED.load(Ordering::Relaxed) == 0 {
        // Nothing anywhere in this process has ever put a named property on a
        // non-object cell, so this receiver cannot have one. The common case,
        // and the reason this is cheap.
        return 0;
    }

    authoritative_has_own(recv, name_ptr, name_len)
}

/// `Object.hasOwn`'s own predicate — the one reflection uses, which answers
/// correctly for every cell kind and needs no readable ShapeId.
///
/// # Safety
/// `recv` is any NaN-boxed value; `name_ptr`/`name_len` name this call's
/// method.
unsafe fn authoritative_has_own(recv: f64, name_ptr: *const u8, name_len: usize) -> i32 {
    if name_ptr.is_null() || name_len == 0 {
        return 1;
    }
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    if key.is_null() {
        return 1;
    }
    let key_value = f64::from_bits(crate::JSValue::string_ptr(key).bits());
    // The authoritative predicate — the one behind `Object.hasOwn` — which
    // already answers correctly for every cell kind (the `hasOwn` rows of
    // `test_parity_own_override_beats_builtin.ts` pass on unfixed main; only
    // the CALL disagreed). Asking it, rather than re-deriving own-ness from a
    // shape descriptor, is the point: it does not need a readable ShapeId, and
    // a Map/Set/Array cell's `+4` word is `capacity`, not one.
    let has_own = crate::object::object_ops::has_own::js_object_has_own(recv, key_value);
    i32::from(has_own.to_bits() == crate::value::TAG_TRUE)
}

/// The receiver's own USER method of this name, if it has one.
///
/// `None` means "run the builtin": either nothing owns the name, or what owns
/// it is a BORROWED builtin (`m.get = Map.prototype.get`), which must take the
/// native arm — dispatching it by name again is how an earlier attempt at
/// #10943 recursed until the stack ran out. `array::generic`'s
/// `object_owns_user_method` identifies that borrowed case. An own
/// non-callable value must instead throw before the kind dispatcher runs.
///
/// # Safety
/// `recv` is any NaN-boxed value; `name` is this call's method name.
pub(crate) unsafe fn own_user_method_value(recv: f64, name: &str) -> Option<f64> {
    // The same relaxed arm the emitted guard consults: nothing anywhere has
    // ever put a named property on a non-object cell, so nothing can shadow.
    if PERRY_OWN_NAMED_PROP_INSTALLED.load(Ordering::Relaxed) == 0 {
        return None;
    }
    resolve_own_user_method(recv, name)
}

/// [`own_user_method_value`] without the global install arm, for a caller that
/// has already proven the receiver owns `name` by other means.
///
/// The array push arm is that caller (#11021): an array records its own named
/// properties in its header and its own side tables, and not every install
/// path passes the exotic-store gauntlet that arms
/// [`PERRY_OWN_NAMED_PROP_INSTALLED`] — `js_array_set_string_key` reaches
/// `array_named_property_set` directly. Consulting the global arm there would
/// be the flags-cannot-describe-the-receiver bug again, one layer down.
///
/// # Safety
/// As [`own_user_method_value`].
unsafe fn resolve_own_user_method(recv: f64, name: &str) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv = scope.root_nanbox_f64(recv);
    let jsval = crate::JSValue::from_bits(recv.get_nanbox_u64());
    if !jsval.is_pointer() {
        return None;
    }

    // Exotic cells keep their own named properties in the expando table.
    // Other cells need an own-presence check before the general getter: that
    // getter sees Array's own methods but would otherwise walk the prototype
    // and mistake an inherited builtin for an own value.
    let value =
        match crate::object::exotic_expando::exotic_expando_kind_of_value(recv.get_nanbox_f64()) {
            Some((addr, kind)) => f64::from_bits(crate::object::exotic_expando::value_lookup(
                kind, addr, name,
            )?),
            None => {
                if authoritative_has_own(recv.get_nanbox_f64(), name.as_ptr(), name.len()) == 0 {
                    return None;
                }
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let raw = (recv.get_nanbox_u64() & crate::value::POINTER_MASK)
                    as *const crate::object::ObjectHeader;
                crate::object::js_object_get_field_by_name_f64(raw, key)
            }
        };
    // The borrowed-builtin classifier below allocates its key. Keep a callable
    // own value rooted until it returns, then hand the refreshed value to the
    // caller (which roots it before invoking it).
    let value = scope.root_nanbox_f64(value);
    if !crate::object::value_is_callable(value.get_nanbox_f64())
        && !crate::proxy::proxy_wraps_callable(value.get_nanbox_f64())
    {
        crate::error::js_throw_type_error_not_a_function(
            std::ptr::null(),
            0,
            name.as_ptr(),
            name.len(),
        );
    }
    // A borrowed builtin (`m.get = Map.prototype.get`) must keep the native
    // arm: dispatching it by name again is the recursion an earlier attempt
    // hit. The classifier `object_owns_user_method` uses is applied to the
    // value already read, rather than through that function, because it
    // performs the `Get` a second time — and an own ACCESSOR's getter is user
    // code that `Get(O, P)` runs exactly once.
    if !crate::array::value_is_own_user_method(value.get_nanbox_f64(), name) {
        return None;
    }
    Some(value.get_nanbox_f64())
}

/// Call an own USER method in Get-then-Call order, or `None` when the receiver
/// has no own user method of that name (nothing owns it, or what owns it is a
/// borrowed builtin, which must take the native arm).
///
/// THE one implementation. The universal dispatcher calls it whole, and the
/// array push arm ([`js_array_push_f64_spec_or_own`]) calls its two halves —
/// [`resolve_own_user_method`] and [`invoke_own_user_method`] — because it has
/// its own proof that the receiver owns the name. Two copies of this sequence
/// is how the next divergence gets introduced, and the difference between them
/// would be a wrong `this` or an unrooted method value, neither of which a
/// fixture reliably catches.
///
/// `IMPLICIT_THIS` is bound across the call and restored after it, because the
/// callee reads its receiver from there when it has no lexical `this`.
///
/// # Safety
/// `recv` is any NaN-boxed value; `args` are NaN-boxed values live at the call.
pub(crate) unsafe fn call_own_user_method(recv: f64, name: &str, args: &[f64]) -> Option<f64> {
    let own = own_user_method_value(recv, name)?;
    Some(invoke_own_user_method(own, recv, args))
}

/// The Call half of [`call_own_user_method`], for a method value already
/// resolved (and still live) at the call.
///
/// # Safety
/// `own` is a callable NaN-boxed value; `recv` and `args` are NaN-boxed
/// values live at the call.
unsafe fn invoke_own_user_method(own: f64, recv: f64, args: &[f64]) -> f64 {
    let root_scope = crate::gc::RuntimeHandleScope::new();
    let method_handle = root_scope.root_nanbox_f64(own);
    let recv_handle = root_scope.root_nanbox_f64(recv);
    let arg_handles = root_scope.root_nanbox_f64_slice(args);
    // Re-read AFTER rooting: resolving the method can move the heap.
    let refreshed = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let prev_this_scope = crate::gc::RuntimeHandleScope::new();
    let prev_this_h = prev_this_scope.root_nanbox_u64(
        crate::object::this_binding::IMPLICIT_THIS
            .with(|c| c.replace(recv_handle.get_nanbox_f64().to_bits())),
    );
    let result = crate::closure::js_native_call_value(
        method_handle.get_nanbox_f64(),
        refreshed.as_ptr(),
        refreshed.len(),
    );
    crate::object::this_binding::IMPLICIT_THIS.with(|c| c.set(prev_this_h.get_nanbox_u64()));
    result
}

/// Does the array behind `arr` OWN a property named `push`? Returns the LIVE
/// head when it does. Non-allocating, and never runs user code.
///
/// `OBJ_FLAG_ARRAY_DESCRIPTORS` clear is the absence proof, answered from the
/// header the caller's admission mask already read (#11021). Every path that
/// gives an array an own named property arms it: `array_named_property_set`
/// on both of its storages (the pairs reserve and the full-array fallback
/// table — `named_props::mark_array_descriptors`), and every
/// `Object.defineProperty` route (`array_object_ops`). It is monotone, and
/// growth carries it (`js_array_grow` copies `_reserved` to the new head). The
/// one named-property store that does NOT arm it is a regex result's inline
/// reserve, whose key set is fixed (`index`, `input`, `groups`, `indices`);
/// adding any other key materialises the reserve into pairs mode, which arms
/// the flag like every other install.
///
/// With the flag set this asks the two places an array's own `push` can live,
/// exactly as `Object.hasOwn`'s array arm does (`array_own_key_present`): the
/// accessor descriptors and the named properties. Asked by bytes, so an array
/// that merely carries an unrelated named property (`a.foo = 1`) pays two
/// lookups and no allocation.
///
/// Only a `GC_TYPE_ARRAY` receiver is answered. An object-backed Array
/// subclass reaches `js_array_push_f64_spec` only through a declared-type lie
/// (`const a: number[] = new MyArr()`), and keeps that behaviour.
unsafe fn array_owning_push(
    arr: *mut crate::array::ArrayHeader,
) -> Option<*mut crate::array::ArrayHeader> {
    let header = crate::value::addr_class::try_read_gc_header(arr as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    // A forwarded head's own header is a stub's: the flag that matters is the
    // live head's, which is where a later install through any alias landed.
    let live = if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        crate::array::clean_arr_ptr_mut(arr)
    } else {
        if header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS == 0 {
            return None;
        }
        arr
    };
    if live.is_null()
        || crate::array::array_object_flags_resolved(live) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS
            == 0
    {
        return None;
    }
    let owns = crate::object::get_accessor_descriptor(live as usize, "push").is_some()
        || crate::array::array_named_property_get_by_name(live, "push").is_some();
    owns.then_some(live)
}

/// `arr.push(value)` from the slow arms of the `Expr::ArrayPush` lowering, with
/// an own `push` honoured (#11021).
///
/// # Why the inline tier needs no test of its own
///
/// Every inline push tier admits a receiver through a mask over
/// `GcHeader::_reserved` that already includes `OBJ_FLAG_ARRAY_DESCRIPTORS`,
/// and every own-named-property install arms that bit (see
/// [`array_owning_push`]). So an array that owns `push` can never take the
/// inline store: it fails admission and lands in a slow arm, and every slow arm
/// calls THIS. The hot path pays nothing — the absence proof rides the
/// instructions it already spends on `_reserved`.
///
/// # Why a second exit, and not a bail
///
/// `js_array_push_f64_spec` returns the new head, and the expression's value is
/// computed from the array's length afterwards, so a receiver that fails the
/// admission mask and is handed to it still runs the builtin: no bail from it
/// can carry an own method's return value. This entry has two exits instead:
///
/// * `*own_out = 0` and the new head's address — exactly
///   `js_array_push_f64_spec`, which the caller writes back and measures;
/// * `*own_out = 1` and the NaN-boxed bits of the METHOD's return, which the
///   caller's phi takes as the expression's value: no append happened, so
///   there is no head to write back and no length to recompute.
///
/// A borrowed builtin (`a.push = Array.prototype.push`) is not a user method
/// and takes the first exit; an own non-callable value throws `TypeError`
/// before anything is appended, as `Get` then `Call` requires.
///
/// # Safety
/// `arr` is the receiver's unboxed address; `value` is a NaN-boxed value;
/// `own_out` points at a writable `u32`.
#[no_mangle]
pub unsafe extern "C" fn js_array_push_f64_spec_or_own(
    arr: *mut crate::array::ArrayHeader,
    value: f64,
    own_out: *mut u32,
) -> u64 {
    if !own_out.is_null() {
        *own_out = 0;
    }
    // The common case, in the one header probe `js_array_push_f64_spec` makes
    // anyway: a plain live array has `OBJ_FLAG_ARRAY_DESCRIPTORS` clear, so it
    // owns no named property at all. Asking `array_owning_push` first cost the
    // local tail — whose EVERY push is this call — a second probe: +46
    // instructions per push on a captured receiver, measured.
    if let Some(head) = crate::array::push_spec_if_plain(arr, value) {
        return head as u64;
    }
    let Some(live) = array_owning_push(arr) else {
        return crate::array::push_spec_declined(arr, value) as u64;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(live as i64));
    let value = scope.root_nanbox_f64(value);
    // Get: resolving the method may run an own accessor's getter, so nothing
    // below may hold a value read before it.
    match resolve_own_user_method(recv.get_nanbox_f64(), "push") {
        Some(method) => {
            let method = scope.root_nanbox_f64(method);
            let result = invoke_own_user_method(
                method.get_nanbox_f64(),
                recv.get_nanbox_f64(),
                &[value.get_nanbox_f64()],
            );
            if !own_out.is_null() {
                *own_out = 1;
            }
            result.to_bits()
        }
        None => {
            let head = (recv.get_nanbox_u64() & crate::value::POINTER_MASK)
                as *mut crate::array::ArrayHeader;
            crate::array::js_array_push_f64_spec(head, value.get_nanbox_f64()) as u64
        }
    }
}

#[cfg(test)]
pub(crate) unsafe fn array_owning_push_for_test(
    arr: *mut crate::array::ArrayHeader,
) -> Option<*mut crate::array::ArrayHeader> {
    array_owning_push(arr)
}

// Called from generated code only, so release/LTO builds may otherwise strip
// the `#[no_mangle]` export.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ARRAY_PUSH_F64_SPEC_OR_OWN: unsafe extern "C" fn(
    *mut crate::array::ArrayHeader,
    f64,
    *mut u32,
) -> u64 = js_array_push_f64_spec_or_own;
