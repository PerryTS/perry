//! `PutValue` for property references (`obj.k = v` / `obj[k] = v` runtime
//! dispatch), split out of `proxy.rs` to keep it under the file-size gate.
//! Routes Proxy traps, integer-indexed exotics, exotic expando cells, and
//! the ordinary receiver-aware `[[Set]]` walk.

use super::*;
use crate::codegen_abi::LayoutWord;

mod dense_array;

/// Receiver-kind test shared by every object-write fast path (#8098).
///
/// The receiver's shape kind supplies the admission: `Ordinary` or
/// `OrdinaryNumericProof` (retired by the store's layout note). Class objects,
/// dictionaries and unmarked native-module receivers keep full `[[Set]]`.
#[inline]
unsafe fn write_fast_path_receiver_kind_ok(
    obj: *const crate::ObjectHeader,
    _obj_flags: u16,
) -> bool {
    crate::object::shapes::store_kind::audit::note_admission_read();
    matches!(
        crate::object::shapes::shape_object_kind_of_receiver(
            (crate::object::shapes::object_shape_stamp(obj)).0
        ),
        Some(
            crate::object::shapes::ShapeObjectKind::Ordinary
                | crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof
        )
    )
}

/// Assignment PutValue for a property reference. Returns the assigned RHS value
/// on success or sloppy failure, and throws TypeError when strict code attempts
/// a failed [[Set]].
#[no_mangle]
pub extern "C" fn js_put_value_set(
    target: f64,
    key: f64,
    value: f64,
    receiver: f64,
    strict: i32,
) -> f64 {
    #[cfg(perry_contract_checks)]
    crate::contracts::put_value(target, key, value, receiver);
    // Short strings and heap strings name the same property. Reuse the
    // existing allocation-free atom resolver before asking the shape, as
    // the transition-only entry did, so a warm short key keeps that answer.
    let key = if crate::JSValue::from_bits(key.to_bits()).is_short_string() {
        unsafe { crate::object::store_key::interned_key_for_store(key) }
            .map(|atom| crate::value::js_nanbox_string(atom as i64))
            .unwrap_or(key)
    } else {
        key
    };
    // Sloppy script assignment lowers to PutValue rather than the named-field
    // setter.  Existing own data fields need none of PutValue's rooting,
    // ToPropertyKey, Proxy, typed-array, or receiver-aware prototype work.
    // Keep this before the handle scope: the helper validates both heap
    // headers and only performs a barriered overwrite when the target and
    // receiver are the same ordinary object and codegen supplied an interned
    // heap-string key.
    // Case 2c: a family object's late key and a key outside its layout
    // (`layout_desc::family_store`); the layout word stays.
    if target.to_bits() == receiver.to_bits()
        && unsafe { crate::object::layout_desc::family_store_value(target, key, value) }
    {
        return value;
    }
    let target_bits = target.to_bits();
    let key_bits = key.to_bits();
    if target_bits == receiver.to_bits()
        && (target_bits & !POINTER_MASK) == POINTER_TAG
        && (key_bits & !POINTER_MASK) == crate::value::STRING_TAG
    {
        let obj = (target_bits & POINTER_MASK) as *mut crate::ObjectHeader;
        let key_ptr = (key_bits & POINTER_MASK) as *const crate::StringHeader;
        // A template private-field slot is not the storage for a fresh
        // evaluation. Consume the guard hint and select its lexical storage
        // before the ordinary overwrite can claim that template slot.
        if !crate::object::field_get_set::cannot_be_private_member_name(key_ptr) {
            let scope = crate::gc::RuntimeHandleScope::new();
            let value_root = scope.root_nanbox_f64(value);
            if crate::object::field_get_set::private_member_set_by_name(obj, key_ptr, value) {
                return value_root.get_nanbox_f64();
            }
            // The private helper rejects ordinary keys before any allocation.
        }
        if unsafe { crate::object::shape_store::try_own_store(obj, key_ptr, value) } {
            return value;
        }
    } else if dense_array::try_set(target, key, value, receiver) {
        // A Number index needs no observable ToPropertyKey. The array's
        // header and slot answer [[Set]] without constructing a String key.
        // The preceding arm's String key cannot hit this lane: named-field
        // misses continue straight to their key-add proof below.
        return value;
    }
    let (target, key, value, receiver) = {
        let mut refreshed = None;
        if target_bits == receiver.to_bits()
            && (target_bits & !POINTER_MASK) == POINTER_TAG
            && (key_bits & !POINTER_MASK) == crate::value::STRING_TAG
        {
            if let Some(stored) = unsafe {
                crate::object::shape_store::try_warm_transition(
                    (target_bits & POINTER_MASK) as *mut crate::ObjectHeader,
                    (key_bits & POINTER_MASK) as *const crate::StringHeader,
                    value,
                    &mut refreshed,
                )
            } {
                return stored;
            }
        }
        match refreshed {
            Some((t, k, v)) => (t, k, v, t),
            None => (target, key, value, receiver),
        }
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let key = key_handle.get_nanbox_f64();
    let property_key_handle =
        scope.root_nanbox_f64(unsafe { crate::object::js_to_property_key(key) });
    let property_key = property_key_handle.get_nanbox_f64();
    // #8495: read the operands AFTER `js_to_property_key`, never before it.
    // ToPropertyKey runs user code (`toString`/`Symbol.toPrimitive`), which
    // allocates and can evacuate. The handles above keep these values ALIVE,
    // but binding them to plain `f64` locals first captures pre-collection
    // addresses that the evacuation then leaves stale — rooting that the
    // rest of the function never benefits from. The observable symptom is a
    // stored value that reads back `undefined` one collection later
    // (`o[heavyKey("k")] = payload()` in
    // `gc_property_key_operand_rooting_6935`), i.e. exactly the #6935
    // contract, re-broken by reading through the handles too early.
    let target = target_handle.get_nanbox_f64();
    let value = value_handle.get_nanbox_f64();
    let receiver = receiver_handle.get_nanbox_f64();

    let (target, property_key, value, receiver) = if target.to_bits() == receiver.to_bits()
        && target.to_bits() & !POINTER_MASK == POINTER_TAG
        && property_key.to_bits() & !POINTER_MASK == crate::value::STRING_TAG
    {
        let mut may_move = false;
        if let Some(stored) = unsafe {
            crate::object::shape_store::try_store_with_refresh(
                target,
                property_key,
                value,
                &mut may_move,
            )
        } {
            return stored;
        }
        if may_move {
            // Marking a prototype or interning a key can collect. Rejected
            // kinds and exceptional keys do neither and need no replay.
            (
                target_handle.get_nanbox_f64(),
                property_key_handle.get_nanbox_f64(),
                value_handle.get_nanbox_f64(),
                receiver_handle.get_nanbox_f64(),
            )
        } else {
            (target, property_key, value, receiver)
        }
    } else {
        (target, property_key, value, receiver)
    };

    if lookup(target).is_none() {
        if set_integer_indexed_exotic(target, property_key, value) {
            return value;
        }
        if target.to_bits() == receiver.to_bits() {
            if let Some(stored) = set_nonindexed_buffer_named_self(target, property_key, value) {
                if !stored && strict != 0 {
                    let key_name =
                        key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
                    crate::error::throw_immutable_write(0, &key_name);
                }
                return value;
            }
        }
        // Integer-Indexed exotic objects: a key that is *not* a CanonicalNumeric
        // index does OrdinarySet, creating/looking-up a normal own property on
        // the typed array (ECMA-262 §10.4.5.5). The generic
        // `ordinary_set_with_receiver` path below mis-reads the typed-array
        // header as an `ObjectHeader` and segfaults, so route typed-array
        // targets to the TA-aware setters (mirroring `js_object_set_field_by_name`).
        // A CanonicalNumeric-but-out-of-bounds key (`"1.5"`, `"NaN"`, `"-0"`)
        // is classified `IntegerIndex` inside `typed_array_set_property_by_name`
        // and silently ignored — never materialized as an ordinary property.
        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(target) {
            if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
                unsafe {
                    crate::symbol::js_object_set_symbol_property(target, property_key, value);
                }
                return value;
            }
            if let Some(name) = key_to_rust_string(property_key) {
                unsafe {
                    crate::typedarray_props::typed_array_set_property_by_name(addr, &name, value);
                }
                return value;
            }
        }
        // Date / Error exotic cells: route to the expando-aware
        // setter — the ordinary path below would bit-cast them. Throws on a
        // rejected strict write. (See `object::exotic_expando`.)
        if let Some(v) = crate::object::exotic_expando::exotic_put_value_set(
            target,
            property_key,
            value,
            receiver,
            strict,
        ) {
            return v;
        }
        // #5437: a live Web Stream handle (a `POINTER_TAG` id in the stream
        // band). React's `renderToReadableStream` attaches its shell-ready
        // promise as an expando (`stream.allReady = ...`); without a store the
        // write was dropped (sloppy) or threw read-only (strict), which killed
        // the Next.js dynamic-SSR render. Route the write to the stdlib
        // per-stream expando table (GC-traced there).
        if let Some(id) = crate::object::stream_handle_id(target) {
            if let (Some(probe), Some(setter)) = (
                crate::object::stream_handle_probe(),
                crate::object::stream_expando_set(),
            ) {
                if unsafe { probe(id) } {
                    if let Some(name) = key_to_rust_string(property_key) {
                        unsafe { setter(id, name.as_ptr(), name.len(), value) };
                    }
                }
            }
            // A stream-band id is a reserved handle, never a settable
            // object — stop here rather than falling through to the
            // ordinary `[[Set]]` walk, even when the expando write was a
            // no-op (dead handle / hooks absent / non-UTF-8 key). Mirrors
            // the `js_object_set_field_by_name` stream guard.
            return value;
        }
        if target.to_bits() == receiver.to_bits() && key_is_length(property_key) {
            // #7574: `sub.length = n` on a `class X extends Array` instance.
            // `array_ptr_from_value` rightly answers `None` (the instance is an
            // `ObjectHeader`, not an `ArrayHeader`), so this fell through to the
            // ordinary set, which recorded the new `length` but left every
            // element at or above it in place — `sub[1]` still read its old
            // value after `sub.length = 1`. Run the Array-exotic
            // `Set(O, "length", n, true)`, which deletes the truncated indices.
            if crate::array::is_array_subclass_value(target) {
                crate::array::array_object_set_length(
                    target,
                    crate::builtins::js_number_coerce(value),
                );
                return value;
            }
            if let Some(arr) = array_ptr_from_value(target) {
                // PutValue(`arr.length = v`) is `Set(O, "length", v, Throw)`. In
                // strict mode a frozen array's non-writable `length` makes the
                // write throw a TypeError instead of silently no-oping.
                if strict != 0 {
                    crate::array::js_array_set_length_strict(arr, value);
                } else {
                    crate::array::js_array_set_length(arr, value);
                }
                return value;
            }
        }
    }

    if target_bits == TAG_NULL || target_bits == TAG_UNDEFINED {
        let key_name = key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
        // Node's wording names which nullish value the base was.
        let base = if target_bits == TAG_NULL {
            "null"
        } else {
            "undefined"
        };
        let msg = format!("Cannot set properties of {base} (setting '{key_name}')");
        return throw_type_error(&msg);
    }
    let ok = if lookup(target).is_some() {
        // A proxy in the prototype chain is the [[Set]] target, but the
        // receiver remains the original assignment receiver. `super.x = v`
        // depends on this distinction just as ordinary inherited writes do.
        proxy_set_with_receiver(target, property_key, value, receiver).to_bits() == TAG_TRUE
    } else {
        // #8082: `ordinary_set_with_receiver` can run user setters (and
        // allocates), so it is a collection point — the raw `receiver` and
        // `property_key` locals go stale across it. The forced-moving gate
        // faulted on exactly this pair inside the subclass-length note's
        // header read. Root both and re-read after the set.
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver_h = scope.root_nanbox_f64(receiver);
        let key_h = scope.root_nanbox_f64(property_key);
        let stored = ordinary_set_with_receiver(target, property_key, value, receiver);
        // #7574: `sub[3] = v` on a `class X extends Array` instance is an
        // Array-exotic `[[DefineOwnProperty]]` and must leave `length == 4`.
        // Perry models the instance as a plain object, so the ordinary set
        // above records the index but never touches `length` — pre-fix
        // `sub[0] = 10; sub.length` read back `0` (node: `1`), on the
        // `T[]`-annotated and the unannotated path alike (both land here
        // through `js_put_value_set_ic_miss`). Cheap no-op for every other
        // receiver: an object literal short-circuits on `class_id == 0`.
        if stored {
            crate::array::note_array_subclass_index_write(
                receiver_h.get_nanbox_f64(),
                key_h.get_nanbox_f64(),
            );
        }
        stored
    };
    if !ok && strict != 0 {
        let key_name = key_to_rust_string(property_key).unwrap_or_else(|| "property".to_string());
        let receiver = receiver_handle.get_nanbox_f64();
        if let Some(class_name) = getter_only_accessor_owner_name(receiver, &key_name) {
            crate::collection_iter::throw_type_error(&format!(
                "Cannot set property {key_name} of #<{class_name}> which has only a getter"
            ));
        }
        // Node names a class instance by its class: `#<Account>`.
        if let Some(class_name) = class_instance_display_name(receiver) {
            crate::collection_iter::throw_type_error(&format!(
                "Cannot assign to read only property '{key_name}' of object '#<{class_name}>'"
            ));
        }
        crate::error::throw_immutable_write(0, &key_name);
    }
    value_handle.get_nanbox_f64()
}

/// The class name of a compiled class instance, for error texts; `None` for
/// every other value.
fn class_instance_display_name(receiver: f64) -> Option<String> {
    let recv = crate::JSValue::from_bits(receiver.to_bits());
    if !recv.is_pointer() {
        return None;
    }
    let object = recv.as_pointer::<crate::ObjectHeader>();
    // SAFETY: the header is read through the checked reader, and only an
    // ordinary object is asked for its shape.
    let is_object = unsafe { crate::value::addr_class::try_read_gc_header(object as usize) }
        .is_some_and(|header| crate::gc::gc_type_is_object(header.obj_type));
    if !is_object || !unsafe { crate::object::object_is_shaped(object) } {
        return None;
    }
    let class_id = crate::object::js_object_get_class_id(object);
    if class_id == 0 {
        return None;
    }
    crate::object::class_name_for_id(class_id)
}

/// For a refused strict write: when the key resolves on `receiver`'s chain to
/// an accessor with no setter, the receiver's constructor name for node's
/// "which has only a getter" message. Cold (only on a throw).
#[cold]
fn getter_only_accessor_owner_name(receiver: f64, key: &str) -> Option<String> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cur = scope.root_nanbox_f64(receiver);
    for _ in 0..64 {
        let v = cur.get_nanbox_f64();
        let js = crate::JSValue::from_bits(v.to_bits());
        if !js.is_pointer() {
            return None;
        }
        let addr = js.as_pointer::<u8>() as usize;
        if let Some(acc) = crate::object::get_accessor_descriptor(addr, key) {
            if acc.set != 0 {
                return None;
            }
            let recv = crate::JSValue::from_bits(receiver.to_bits());
            let class_id =
                crate::object::js_object_get_class_id(recv.as_pointer::<crate::ObjectHeader>());
            return Some(
                crate::object::class_name_for_id(class_id).unwrap_or_else(|| "Object".to_string()),
            );
        }
        if crate::object::get_property_attrs(addr, key).is_some() {
            return None;
        }
        cur.set_nanbox_f64(crate::object::js_object_get_prototype_of(v));
    }
    None
}

#[cfg(test)]
#[path = "put_value/record_fixture.rs"]
pub(crate) mod record_fixture;

/// The bit an overflow-held slot carries in a slot word (`read_stub`,
/// `dynamic_key_read`, the named-set fast paths): the slot indexes the
/// receiver's spill/overflow storage, not its inline slots.
pub(crate) const IC_SLOT_OVERFLOW_BIT: u32 = 1 << 30;

/// A same-receiver dynamic-key store `target[key] = value` (object model
/// §3.3; perry-codegen `expr/proxy_reflect.rs`): a stable-tombstone re-add,
/// a key the receiver's layout names (its slot) or any other computed key
/// (appended to the receiver's overflow, no transition), else the complete
/// strict-aware `[[Set]]`. Nothing is remembered.
#[no_mangle]
pub extern "C" fn js_put_value_set_dynamic(target: f64, key: f64, value: f64, strict: i32) -> f64 {
    let key = match own_overwrite_first(target, key, value) {
        Ok(stored) => return stored,
        Err(key) => key,
    };
    // Case 2c: a family object's late key and a key outside its layout
    // (`layout_desc::family_store`); the layout word stays.
    if unsafe { crate::object::layout_desc::family_store_value(target, key, value) } {
        return value;
    }
    // Charter step 5: migrate-on-miss (DESIGN §1.5 step 4).
    crate::object::field_rep_store::migrate_on_miss_value(target.to_bits());
    let target_bits = target.to_bits();
    if (target_bits & !POINTER_MASK) == POINTER_TAG {
        let obj = (target_bits & POINTER_MASK) as *mut crate::ObjectHeader;
        // Stable-tombstone re-adds own their complete rooted transition.
        if let Some((_, _, value)) = crate::object::try_readd_stable_tombstone(obj, key, value) {
            return value;
        }
        // §3.3: a decline allocates nothing, so the raw operands stay
        // current.
        if let Some(stored) =
            unsafe { crate::object::shape_store::try_dynamic_store(obj, key, value) }
        {
            return stored;
        }
    }
    js_put_value_set(target, key, value, target, strict)
}

/// The own-key front of [`js_put_value_set_dynamic`]. An own writable data
/// property is the answer of [[Set]] for any receiver (OrdinarySet finds the
/// own property first), so a receiver's own keys are asked before the
/// add-path probes: the shape record of a keyed ordinary layout, or the
/// overflow of an overflowed receiver (it lists every own key, §3.4). The
/// key is resolved to its atom only for a receiver that has own keys.
/// `Ok` is the stored value; `Err` carries the key the add path continues
/// with (its atom once resolved: the same property, so the steps after this
/// do not resolve it again).
#[inline(always)]
fn own_overwrite_first(target: f64, key: f64, value: f64) -> Result<f64, f64> {
    use crate::object::shape_store::{self, OwnOverwrite};
    if (target.to_bits() & !POINTER_MASK) != POINTER_TAG {
        return Err(key);
    }
    let obj = (target.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader;
    match unsafe { shape_store::own_overwrite_holder(obj) } {
        Some(OwnOverwrite::Record(record)) => {
            let atom = if crate::JSValue::from_bits(key.to_bits()).is_short_string() {
                unsafe { crate::object::store_key::interned_key_for_store(key) }
            } else {
                unsafe { crate::object::store_key::interned_heap_key(key) }
            };
            let Some(atom) = atom else {
                return Err(key);
            };
            if unsafe { shape_store::try_record_own_overwrite(obj, record, atom, value) } {
                return Ok(value);
            }
            Err(crate::value::js_nanbox_string(atom as i64))
        }
        Some(OwnOverwrite::Overflow) => unsafe {
            shape_store::try_overflow_own_overwrite(obj, key, value)
        },
        None => Err(key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Object model step 4 (§3.3): a computed key the receiver's layout does
    /// not name is appended to the receiver's overflow. The layout id never
    /// changes (the shadow bit marks the overflow) and no shape transition is
    /// learned or reused; a user-class receiver gets the same value
    /// semantics.
    #[test]
    fn a_dynamic_store_adds_an_unnamed_key_to_the_overflow_without_transitions() {
        let _lock = crate::gc::global_side_table_test_lock();
        crate::object::test_reset_transition_fast_hits();
        crate::object::shapes::test_reset_cached_transition_stamps();

        // Longer than the SSO limit so this exercises the heap-string arm.
        // A unique name keeps other tests' keys out of it.
        let key_bytes = b"__dyn_ic_transition_fast_test_8783";
        let key_ptr =
            crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let key_handle = scope.root_nanbox_f64(crate::value::js_nanbox_string(key_ptr as i64));
        const ANON_CLASS_ID: u32 = 0x8783_1001;
        const USER_CLASS_ID: u32 = 0x8783_1002;
        unsafe { crate::object::declarations::test_declare_anonymous(ANON_CLASS_ID) };
        let first_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(ANON_CLASS_ID, 0));
        let second_handle =
            scope.root_raw_mut_ptr(crate::object::js_object_alloc(ANON_CLASS_ID, 0));
        let user_class_handle =
            scope.root_raw_mut_ptr(crate::object::js_object_alloc(USER_CLASS_ID, 0));
        let read = |obj: *mut crate::ObjectHeader| {
            let key_ptr = (key_handle.get_nanbox_f64().to_bits() & POINTER_MASK)
                as *const crate::StringHeader;
            crate::object::js_object_get_field_by_name(obj, key_ptr).as_number()
        };

        for (handle, value) in [(&first_handle, 11.0), (&second_handle, 29.0)] {
            let obj = handle.get_raw_mut_ptr::<crate::ObjectHeader>();
            let before = unsafe { crate::object::shapes::object_shape_stamp(obj) };
            assert!(!unsafe { crate::object::overflow::has_overflow(obj) });
            assert_eq!(
                js_put_value_set_dynamic(
                    crate::value::js_nanbox_pointer(obj as i64),
                    key_handle.get_nanbox_f64(),
                    value,
                    1,
                ),
                value
            );
            let obj = handle.get_raw_mut_ptr::<crate::ObjectHeader>();
            assert!(
                unsafe { crate::object::overflow::has_overflow(obj) },
                "the computed key is an overflow entry"
            );
            let after = unsafe { crate::object::shapes::object_shape_stamp(obj) };
            assert_eq!(
                crate::object::layout_desc::layout_id_of_word(after.0),
                crate::object::layout_desc::layout_id_of_word(before.0),
                "the layout id does not change"
            );
            assert_ne!(after.0 & crate::object::overflow::SHADOW_BIT, 0);
            assert_eq!(read(obj), value);
        }
        assert_eq!(crate::object::test_transition_fast_hits(), 0);
        assert_eq!(crate::object::shapes::test_cached_transition_stamps(), 0);
        // The first receiver's entry is its own.
        assert_eq!(
            read(first_handle.get_raw_mut_ptr::<crate::ObjectHeader>()),
            11.0
        );

        let user_class = user_class_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
        assert_eq!(
            js_put_value_set_dynamic(
                crate::value::js_nanbox_pointer(user_class as i64),
                key_handle.get_nanbox_f64(),
                47.0,
                1,
            ),
            47.0
        );
        assert_eq!(
            read(user_class_handle.get_raw_mut_ptr::<crate::ObjectHeader>()),
            47.0
        );
        assert_eq!(crate::object::test_transition_fast_hits(), 0);
    }
}

#[cold]
fn trace_object_array_numeric_write_rejection(reason: &'static str) {
    if std::env::var_os("PERRY_TRACE_OBJECT_ARRAY_WRITE_GUARD").is_some() {
        eprintln!("PERRY_OBJECT_ARRAY_WRITE_GUARD_REJECT: {reason}");
    }
}

#[inline]
fn trace_object_array_numeric_write_stage<T>(value: Option<T>, reason: &'static str) -> Option<T> {
    if value.is_none() {
        trace_object_array_numeric_write_rejection(reason);
    }
    value
}

/// Prove all receivers and return up to four inline numeric slot indexes.
///
/// The caller performs one bounded scan, then holds raw array/object pointers
/// for a finite, call-free loop nest. Consequently this helper must reject any
/// receiver, key, or layout state that could require ordinary `[[Set]]`
/// semantics. The fixed array is stack-only; no descriptor allocation is
/// introduced on the preflight path.
fn object_array_numeric_write_slots(
    array: f64,
    keys: &[f64],
    receiver_start: u32,
    receiver_end: u32,
) -> Option<[u16; 4]> {
    // Reuse the process gate js_gc_init disables for typed-feedback tracing,
    // typed-layout verification, and the explicit inline-field escape hatch.
    // This loop bypasses the same observations/checks as the class-field
    // inline clone and therefore must honor the identical gate.
    if keys.is_empty() || keys.len() > 4 || crate::typed_feedback::typed_feedback_trace_requested()
    {
        trace_object_array_numeric_write_rejection("disabled gate or invalid field/count bound");
        return None;
    }

    let array_bits = array.to_bits();
    if (array_bits & !POINTER_MASK) != POINTER_TAG {
        trace_object_array_numeric_write_rejection("receiver container is not an array pointer");
        return None;
    }
    let array_addr = (array_bits & POINTER_MASK) as usize;
    let array_gc = trace_object_array_numeric_write_stage(
        unsafe { crate::value::addr_class::try_read_gc_header(array_addr) },
        "array header is unavailable",
    )?;
    if array_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || array_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || crate::array::array_has_element_descriptors(
            array_addr as *const crate::array::ArrayHeader,
        )
        || crate::array::array_index_fast_path_invalid_for(array_gc._reserved)
    {
        trace_object_array_numeric_write_rejection(
            "array kind, forwarding, descriptor, or index-fast-path state",
        );
        return None;
    }

    let arr = array_addr as *const crate::array::ArrayHeader;
    let (length, capacity) = unsafe { ((*arr).length, (*arr).capacity) };
    // #6812 dynamic-bound loops (`i < arr.length`): the u32::MAX sentinel
    // resolves to the VALIDATED array's own length — after the array-kind
    // checks above, before the 16M cap below, so the emitter's fast nest
    // (which loads the same header length after guard-ok) can never outrun
    // the proven prefix. An empty array is not worth the fast path.
    let receiver_end = if receiver_end == u32::MAX {
        length
    } else {
        receiver_end
    };
    if receiver_start >= receiver_end {
        trace_object_array_numeric_write_rejection("empty or reversed receiver range");
        return None;
    }
    if length > 16_000_000 || capacity > 16_000_000 || length > capacity || receiver_end > length {
        trace_object_array_numeric_write_rejection("array length/capacity/prefix bound");
        return None;
    }

    // Unlike the per-site PIC, this proof does not retain key identity after
    // the call or index a pointer-keyed cache. `find_slot` performs only
    // allocation-free byte comparisons, so a live non-forwarded string-pool
    // handle is sufficient; requiring INTERNED here made eligibility depend
    // accidentally on whether some unrelated site had interned the same name.
    let decode_key = |boxed: f64| -> Option<*const crate::StringHeader> {
        let bits = boxed.to_bits();
        if (bits & !POINTER_MASK) != crate::value::STRING_TAG {
            return None;
        }
        let ptr = (bits & POINTER_MASK) as *const crate::StringHeader;
        let gc = unsafe { crate::value::addr_class::try_read_gc_header(ptr as usize) }?;
        (gc.obj_type == crate::gc::GC_TYPE_STRING
            && gc.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0)
            .then_some(ptr)
    };
    let mut decoded_keys = [std::ptr::null(); 4];
    for (index, boxed) in keys.iter().enumerate() {
        decoded_keys[index] = trace_object_array_numeric_write_stage(
            decode_key(*boxed),
            "target key is not a live heap string",
        )?;
    }

    const BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;

    unsafe fn validated_object(
        bits: u64,
    ) -> Option<(
        *mut crate::ObjectHeader,
        u32,
        *mut crate::array::ArrayHeader,
        u32,
        u32,
    )> {
        if (bits & !POINTER_MASK) != POINTER_TAG {
            return None;
        }
        let addr = (bits & POINTER_MASK) as usize;
        let gc = crate::value::addr_class::try_read_gc_header(addr)?;
        if !crate::gc::gc_type_is_object(gc.obj_type)
            || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || gc._reserved & BLOCKING_FLAGS != 0
        {
            return None;
        }
        let obj = addr as *mut crate::ObjectHeader;
        if !write_fast_path_receiver_kind_ok(obj, gc._reserved) {
            return None;
        }
        let shape = crate::object::shapes::object_shape_descriptor(obj)?;
        if !shape.object_kind.is_ordinary_layout() {
            return None;
        }
        let shape_id = crate::object::shapes::object_shape_stamp(obj);
        let keys = shape.keys as usize as *mut crate::array::ArrayHeader;
        if keys.is_null() || (keys as u64) >> 48 != 0 {
            return None;
        }
        let keys_gc = crate::value::addr_class::try_read_gc_header(keys as usize)?;
        if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        Some((
            obj,
            shape_id.0,
            keys,
            shape.logical_key_count,
            shape.live_inline_slot_count,
        ))
    }

    unsafe fn find_slot(
        keys: *mut crate::array::ArrayHeader,
        key_count: u32,
        key: *const crate::StringHeader,
    ) -> Option<u32> {
        if key_count > 4096 {
            return None;
        }
        crate::object::keys_find_slot_by_key_ptr(crate::object::ObjectKeys::new(keys, key_count), key_count, key)
            .map(|position| crate::object::key_attrs::key_storage_slot(keys, position))
    }

    let elements = unsafe {
        crate::array::array_elements_ptr(arr as *const crate::array::ArrayHeader) as *const f64
    };
    let first_bits = unsafe { (*elements.add(receiver_start as usize)).to_bits() };
    if first_bits == crate::value::TAG_HOLE {
        trace_object_array_numeric_write_rejection("first receiver is a hole");
        return None;
    }
    let (first, shared_shape_id, shared_keys, shared_key_count, first_limit) =
        trace_object_array_numeric_write_stage(
            unsafe { validated_object(first_bits) },
            "first receiver is not an eligible regular shared-shape object",
        )?;
    let shared_rep = crate::object::field_rep_store::shape_rep(LayoutWord(shared_shape_id));
    let mut slots = [0u16; 4];
    for index in 0..keys.len() {
        // `find_slot` caps the shared keys array at 4096 entries, so every
        // non-zero-encoded index fits comfortably in one 16-bit result lane.
        let slot = trace_object_array_numeric_write_stage(
            unsafe { find_slot(shared_keys, shared_key_count, decoded_keys[index]) },
            "target key is absent from the shared shape",
        )?;
        // A finite Number preserves Any/F64, but contradicts a ConstFn body
        // fact. The clone writes without the checked slot funnel, so SPECIAL
        // targets must take the ordinary loop before any slot is published.
        if crate::object::field_rep::slot_rep(shared_rep, slot)
            == crate::object::field_rep::REP_SPECIAL
        {
            trace_object_array_numeric_write_rejection("target slot carries a SPECIAL fact");
            return None;
        }
        slots[index] = trace_object_array_numeric_write_stage(
            u16::try_from(slot).ok(),
            "target slot cannot be encoded",
        )?;
    }

    // #6812 spill lanes: classify each lane as INLINE (slot within the
    // receiver's inline alloc_limit) or SPILL (slot lives in the
    // object-owned overflow buffer, `meta.spill`). The FIRST receiver fixes
    // each lane's mode; every later receiver must be on the SAME side of its
    // own alloc_limit and pass the same coverage proof, so the emitter's
    // per-lane store sequence (raw inline store vs the meta → spill
    // indirection) is uniform across the whole proven prefix. A spill lane
    // sets bit 15 of its result lane; the caller's `+1` packing cannot carry
    // into it (slot ≤ 4096).
    unsafe fn spill_lane_covers(obj: *const crate::ObjectHeader, slot: u32) -> bool {
        let meta = (*obj).meta;
        if meta.is_null() {
            return false;
        }
        let spill = (*meta).spill as *const crate::array::ArrayHeader;
        if spill.is_null() {
            return false;
        }
        // Mirror the shared-keys validation: a live, non-forwarded plain
        // array whose length high-water covers the slot (a key present in
        // the shape implies its value slot was written, so length > slot —
        // verified rather than assumed).
        let Some(gc) = crate::value::addr_class::try_read_gc_header(spill as usize) else {
            return false;
        };
        if gc.obj_type != crate::gc::GC_TYPE_ARRAY
            || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        slot < (*spill).length && slot < (*spill).capacity
    }

    let mut lane_spill = [false; 4];
    for index in 0..keys.len() {
        let slot = u32::from(slots[index]);
        if slot >= first_limit {
            if !unsafe { spill_lane_covers(first, slot) } {
                trace_object_array_numeric_write_rejection(
                    "first receiver target slot is out of bounds",
                );
                return None;
            }
            lane_spill[index] = true;
        }
    }

    for i in (receiver_start as usize + 1)..receiver_end as usize {
        let bits = unsafe { (*elements.add(i)).to_bits() };
        if bits == crate::value::TAG_HOLE {
            trace_object_array_numeric_write_rejection("receiver prefix contains a hole");
            return None;
        }
        let (obj, receiver_shape_id, _object_keys, _object_key_count, limit) =
            trace_object_array_numeric_write_stage(
                unsafe { validated_object(bits) },
                "receiver prefix contains an ineligible object",
            )?;
        if receiver_shape_id != shared_shape_id {
            trace_object_array_numeric_write_rejection(
                "receiver prefix does not share one ShapeId",
            );
            return None;
        }
        for index in 0..keys.len() {
            let slot = u32::from(slots[index]);
            if lane_spill[index] {
                // Mode uniformity: this receiver must ALSO hold the slot in
                // its spill buffer (a wider receiver holding it inline would
                // make the emitter's spill store write the wrong memory).
                if slot < limit || !unsafe { spill_lane_covers(obj, slot) } {
                    trace_object_array_numeric_write_rejection(
                        "receiver prefix contains an out-of-bounds target slot",
                    );
                    return None;
                }
            } else if slot >= limit {
                trace_object_array_numeric_write_rejection(
                    "receiver prefix contains an out-of-bounds target slot",
                );
                return None;
            }
        }
    }

    for index in 0..keys.len() {
        if lane_spill[index] {
            slots[index] |= 0x8000;
        }
    }
    Some(slots)
}

/// #6812 (w12): preflight for the TABLE-DRIVEN write lane
/// (`o[K[idx]] = v`). Validates the key table (live array, length covers
/// the proven index range, first `required_len` entries are strings),
/// materializes each entry to a stable heap string (SSO elements allocate,
/// so the array/table/keys are rooted across materialization), then runs
/// the SAME receiver-prefix proof as the numeric guard via
/// `object_array_numeric_write_slots` — spill lanes included. On success,
/// writes each resolved lane into `out_lanes` in the emitter's encoding
/// (`(slot + 1) | spill_flag`) and returns 1.
#[no_mangle]
pub extern "C" fn js_object_array_keytable_write_guard(
    array: f64,
    key_table: f64,
    required_len: u32,
    receiver_count: u32,
    out_lanes: *mut i64,
) -> i32 {
    if !(1..=4).contains(&required_len) || out_lanes.is_null() {
        return 0;
    }
    let kt_bits = key_table.to_bits();
    if (kt_bits & !POINTER_MASK) != POINTER_TAG {
        return 0;
    }
    let kt_addr = (kt_bits & POINTER_MASK) as usize;
    let Some(kt_gc) = (unsafe { crate::value::addr_class::try_read_gc_header(kt_addr) }) else {
        return 0;
    };
    if kt_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || kt_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return 0;
    }
    if unsafe { (*(kt_addr as *const crate::array::ArrayHeader)).length } < required_len {
        return 0;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let array_handle = scope.root_nanbox_f64(array);
    let table_handle = scope.root_nanbox_f64(key_table);
    let mut key_handles = Vec::with_capacity(required_len as usize);
    for i in 0..required_len {
        let table_now = table_handle.get_nanbox_f64();
        let table_ptr = (table_now.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader;
        let elem = crate::array::js_array_get(table_ptr, i);
        let elem_f64 = f64::from_bits(elem.bits());
        // Stable heap pointer for SSO/heap strings alike; may allocate, so
        // every prior key stays rooted and the table re-reads per element.
        let heap_ptr = crate::value::js_get_string_pointer_unified(elem_f64);
        if heap_ptr == 0 {
            return 0;
        }
        let boxed = f64::from_bits(crate::value::js_nanbox_string(heap_ptr).to_bits());
        key_handles.push(scope.root_nanbox_f64(boxed));
    }
    let keys: Vec<f64> = key_handles.iter().map(|h| h.get_nanbox_f64()).collect();
    let Some(slots) =
        object_array_numeric_write_slots(array_handle.get_nanbox_f64(), &keys, 0, receiver_count)
    else {
        return 0;
    };
    for i in 0..required_len as usize {
        let lane = slots[i];
        let low = (lane & 0x7FFF) as i64 + 1;
        let flag = (lane & 0x8000) as i64;
        unsafe {
            *out_lanes.add(i) = low | flag;
        }
    }
    1
}

/// Preflight for codegen's bounded call-free nested object-write loop.
///
/// Each active slot is encoded as `slot + 1` in a 16-bit lane, with the first
/// key in the least-significant lane. Zero means that the generated raw loop
/// must not run. The caller scans once, then performs only finite numeric
/// stores, whose bits are valid in both raw-f64 and ordinary numeric JSValue
/// fields, until both loops finish. That call-free interval is load-bearing:
/// no GC can move the array, its elements, their shared keys array, or their
/// shapes after this function validates them. A finite double is a canonical
/// value for an `F64` lane and an ordinary Number for an `Any` lane, so no
/// lane of the receivers' shapes can reject these stores.
#[no_mangle]
pub extern "C" fn js_object_array_numeric_write_guard(
    array: f64,
    key_1: f64,
    key_2: f64,
    key_3: f64,
    key_4: f64,
    field_count: u32,
    receiver_count: u32,
) -> u64 {
    if !(1..=4).contains(&field_count) {
        return 0;
    }
    let keys = [key_1, key_2, key_3, key_4];
    let Some(slots) =
        object_array_numeric_write_slots(array, &keys[..field_count as usize], 0, receiver_count)
    else {
        return 0;
    };
    slots[..field_count as usize]
        .iter()
        .enumerate()
        .fold(0, |packed, (index, slot)| {
            packed | ((u64::from(*slot) + 1) << (index * 16))
        })
}

/// Range variant for a constant non-zero inner loop start. The result has the
/// same packed-lane ABI as [`js_object_array_numeric_write_guard`], but proves
/// exactly `receiver_start..receiver_end`; generated code retains the array
/// base pointer and uses the original absolute element index.
#[no_mangle]
pub extern "C" fn js_object_array_numeric_write_range_guard(
    array: f64,
    key_1: f64,
    key_2: f64,
    key_3: f64,
    key_4: f64,
    field_count: u32,
    receiver_start: u32,
    receiver_end: u32,
) -> u64 {
    if !(1..=4).contains(&field_count) {
        return 0;
    }
    let keys = [key_1, key_2, key_3, key_4];
    let Some(slots) = object_array_numeric_write_slots(
        array,
        &keys[..field_count as usize],
        receiver_start,
        receiver_end,
    ) else {
        return 0;
    };
    slots[..field_count as usize]
        .iter()
        .enumerate()
        .fold(0, |packed, (index, slot)| {
            packed | ((u64::from(*slot) + 1) << (index * 16))
        })
}

/// Preserve the #6811 internal ABI for cached generated objects. New codegen
/// uses [`js_object_array_numeric_write_guard`], but an object cache entry
/// produced before the runtime rebuild may still reference this symbol.
#[no_mangle]
pub extern "C" fn js_object_array_numeric_write2_guard(
    array: f64,
    key_1: f64,
    key_2: f64,
    receiver_count: u32,
) -> u64 {
    let Some(slots) = object_array_numeric_write_slots(array, &[key_1, key_2], 0, receiver_count)
    else {
        return 0;
    };
    (u64::from(slots[1]) + 1) << 32 | (u64::from(slots[0]) + 1)
}

#[cfg(test)]
#[path = "put_value/numeric_write_constfn_tests.rs"]
mod numeric_write_constfn_tests;
