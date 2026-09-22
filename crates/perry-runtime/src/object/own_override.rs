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

use std::sync::atomic::{AtomicBool, Ordering};

/// Has any non-`ObjectHeader` cell ever taken a named property?
///
/// Set-only. See the module docs for why it is armed early and never cleared.
static EXOTIC_OWN_NAMED_PROP_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Armed from the top of `field_set_by_name`'s exotic-store gauntlet.
#[inline]
pub(crate) fn note_exotic_named_prop_install() {
    // Relaxed is enough: a stale `false` can only be read by a thread that has
    // not yet observed the store, and that thread's own receivers cannot be
    // the one just written (the write happens-before any publication of the
    // receiver to another thread — perry workers deep-copy rather than share
    // `ObjectHeader`s, #6185).
    if !EXOTIC_OWN_NAMED_PROP_INSTALLED.load(Ordering::Relaxed) {
        EXOTIC_OWN_NAMED_PROP_INSTALLED.store(true, Ordering::Relaxed);
    }
}

/// Test-only: read the arm state.
#[cfg(test)]
pub(crate) fn test_exotic_named_prop_installed() -> bool {
    EXOTIC_OWN_NAMED_PROP_INSTALLED.load(Ordering::Relaxed)
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
            return i32::from(header._reserved & crate::gc::GC_ARRAY_NAMED_PROPS != 0);
        }
    } else {
        // No readable GC header. Cannot prove absence — decline.
        return 1;
    }

    if !EXOTIC_OWN_NAMED_PROP_INSTALLED.load(Ordering::Relaxed) {
        // Nothing anywhere in this process has ever put a named property on a
        // non-object cell, so this receiver cannot have one. The common case,
        // and the reason this is cheap.
        return 0;
    }

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
/// `object_owns_user_method` is the existing two-valued classifier and this
/// asks it rather than repeating the rule.
///
/// # Safety
/// `recv` is any NaN-boxed value; `name` is this call's method name.
pub(crate) unsafe fn own_user_method_value(recv: f64, name: &str) -> Option<f64> {
    // The same relaxed arm the emitted guard consults: nothing anywhere has
    // ever put a named property on a non-object cell, so nothing can shadow.
    if !EXOTIC_OWN_NAMED_PROP_INSTALLED.load(Ordering::Relaxed) {
        return None;
    }
    let jsval = crate::JSValue::from_bits(recv.to_bits());
    if !jsval.is_pointer() {
        return None;
    }

    // Read the OWN property from the table that actually holds it. Neither
    // general getter is right here, and each is wrong in its own direction —
    // both measured on this issue's differential:
    //
    //  * `js_object_get_field_by_name_f64` walks the PROTOTYPE CHAIN, so on a
    //    Set it returned `Set.prototype.has` and the dispatcher called the
    //    builtin thunk (traced: js_native_call_value -> set_proto_has_thunk);
    //  * `js_object_get_own_field_or_undef` is own-only but does not consult
    //    the exotic side table, so on a Map it answered `undefined` and every
    //    Map row regressed.
    //
    // An exotic cell keeps its own named properties in the expando table —
    // the same one `hasOwn`, `typeof` and `Object.keys` read, which is why
    // reflection already agreed with node while the call did not.
    let value = match crate::object::exotic_expando::exotic_expando_kind_of_value(recv) {
        Some((addr, kind)) => {
            f64::from_bits(crate::object::exotic_expando::value_lookup(kind, addr, name)?)
        }
        None => crate::object::object_ops::js_object_get_own_field_or_undef(
            recv,
            name.as_ptr(),
            name.len(),
        ),
    };
    if !crate::JSValue::from_bits(value.to_bits()).is_pointer() {
        return None;
    }
    // A borrowed builtin (`m.get = Map.prototype.get`) must keep the native
    // arm: dispatching it by name again is the recursion an earlier attempt
    // hit. `object_owns_user_method` is the existing two-valued classifier.
    if !crate::array::object_owns_user_method(recv, name) {
        return None;
    }
    Some(value)
}
