//! `delete obj.x` and object-rest (`{...rest}`) semantics:
//! `js_object_delete_field`, `js_object_delete_dynamic`, `js_object_rest`.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation.

use super::*;

/// Box a `delete` operation's success bit into a JS boolean, throwing a
/// `TypeError` in strict mode when the delete was refused (`deleted == 0`,
/// i.e. the property exists and is non-configurable). Per spec, a strict-mode
/// `delete` whose `[[Delete]]` returns `false` throws; sloppy mode yields
/// `false`. `deleted != 0` always yields `true`.
#[no_mangle]
pub extern "C" fn js_delete_result(deleted: i32, strict: i32) -> f64 {
    if deleted == 0 {
        if strict != 0 {
            let message = "Cannot delete property";
            let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
            let err = crate::error::js_typeerror_new(msg);
            crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
        }
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    f64::from_bits(crate::value::TAG_TRUE)
}

/// Delete a field from an object by its string key name
/// Returns 1 if the field was deleted (or didn't exist), 0 otherwise
#[no_mangle]
pub extern "C" fn js_object_delete_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> i32 {
    if obj.is_null() || key.is_null() {
        return 1;
    }
    if let Some(result) = crate::process::process_env_delete_field(obj, key) {
        return result;
    }
    // A delete can rewrite key→slot mappings in place (same keys_array
    // address), so cached (keys_array, key)→index plans must be flushed
    // (`object::prop_plan` read-plan cache).
    super::prop_plan::prop_plan_epoch_bump();
    // A Proxy is a small registered id in the proxy id band, not a heap
    // ObjectHeader. Dereferencing it below (GC header / keys_array reads) would
    // segfault. Route `delete proxy.k` / `delete proxy[k]` through the proxy
    // `deleteProperty` trap. (#2846-family Proxy crash cluster.)
    {
        let addr = obj as u64;
        if crate::value::addr_class::is_proxy_id_band(addr as usize) {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                let r = crate::proxy::js_proxy_delete(boxed, key_f64);
                return if crate::value::js_is_truthy(r) != 0 {
                    1
                } else {
                    0
                };
            }
        }
    }
    // The hand-typed 0x10000 floor here was an order of magnitude below the real
    // boundary (HANDLE_BAND_MAX = 0x100000), so the fetch (0x40000..0xE0000) and
    // zlib (0xE0000..0xF0000) handle bands fell through to the heap path below
    // and got dereferenced -> SIGSEGV on Linux. Use the centralized predicate.
    if crate::value::addr_class::is_handle_band(obj as usize) {
        unsafe {
            if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                let class_id = obj as usize as u32;
                if super::class_registry::class_name_for_id(class_id).is_some() {
                    super::class_registry::class_delete_own_dynamic_prop(class_id, name);
                    super::class_registry::class_mark_key_deleted(class_id, name);
                }
                // #6363: a native HANDLE's own properties are its user expandos.
                // `delete` used to unconditionally report success while LEAVING
                // the property in place — `delete headers.foo` returned true and
                // `headers.foo` still read back its old value. Actually remove
                // it, and reject (false) a non-configurable one, matching
                // ordinary `[[Delete]]`. An absent key still reports true, which
                // is what `delete request.__nope` must do (Node: true).
                return i32::from(super::handle_expando::handle_expando_delete(
                    obj as usize as i64,
                    name,
                ));
            }
        }
        return 1;
    }
    unsafe {
        if let Some(addr) =
            crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
        {
            return crate::typedarray_props::typed_array_delete_own_property(
                addr as *mut crate::typedarray::TypedArrayHeader,
                key,
            );
        }
        // ArrayBuffer / SharedArrayBuffer / DataView are registered
        // BufferHeaders with ordinary named expandos.  They must not fall
        // through to the ObjectHeader keys-array walk.
        if crate::buffer::is_registered_buffer(obj as usize) {
            if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                if let Some(attrs) = get_property_attrs(obj as usize, name) {
                    if !attrs.configurable() {
                        return 0;
                    }
                }
                crate::buffer::buffer_delete_own_prop(obj as usize, name);
                super::clear_accessor_descriptor(obj as usize, name);
                super::clear_property_attrs(obj as usize, name);
            }
            return 1;
        }
        if let Some(result) = super::arguments_object_before_delete(obj, key) {
            return result;
        }
        // Date / RegExp / Error exotic instances: expando props live in side
        // tables; the keys_array scan below would bit-cast the cell. Builtin
        // own slots (`lastIndex`) are non-configurable → delete fails.
        if let Some(kind) = super::exotic_expando::exotic_expando_kind(obj as usize) {
            use super::exotic_expando::ExoticKind;
            if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                if let Some(attrs) = get_property_attrs(obj as usize, name) {
                    if !attrs.configurable() {
                        return 0;
                    }
                }
                if kind == ExoticKind::RegExp && name == "lastIndex" {
                    return 0;
                }
                super::exotic_expando::value_remove(kind, obj as usize, name);
                if kind == ExoticKind::Error {
                    crate::error::js_error_delete_builtin_own_property(
                        obj as *mut crate::error::ErrorHeader,
                        name,
                    );
                }
                super::clear_accessor_descriptor(obj as usize, name);
                super::clear_property_attrs(obj as usize, name);
            }
            return 1;
        }
        if (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                    // An Array's `length` is a non-configurable exotic own
                    // property with no descriptor-table entry, so the
                    // `get_property_attrs` check below misses it. `delete
                    // arr.length` must report failure (throws in strict mode).
                    if name == "length" {
                        return 0;
                    }
                    if let Some(attrs) = get_property_attrs(obj as usize, name) {
                        if !attrs.configurable() {
                            return 0;
                        }
                    }
                    if let Some(index) = super::canonical_array_index(name) {
                        return crate::array::js_array_delete(
                            obj as *mut crate::array::ArrayHeader,
                            index,
                        );
                    }
                    // Named (non-index) property: drop the value-store entry AND
                    // any accessor / attribute side-table state. A named
                    // accessor (`Object.defineProperty(arr, "p", {get})`) lives
                    // ONLY in the side tables, so without these clears the
                    // delete was a no-op and `hasOwnProperty("p")` stayed true
                    // (test262 verifyProperty's configurable check deletes then
                    // asserts the key is gone).
                    crate::array::array_named_property_delete(
                        obj as *const crate::array::ArrayHeader,
                        key,
                    );
                    super::clear_accessor_descriptor(obj as usize, name);
                    super::clear_property_attrs(obj as usize, name);
                    return 1;
                }
                crate::array::array_named_property_delete(
                    obj as *const crate::array::ArrayHeader,
                    key,
                );
                return 1;
            }
        }
        // #3655: `delete fn.name` / `delete fn.userProp`. Functions/closures
        // aren't `ObjectHeader`s — reading `keys_array` off one is out of
        // bounds. The built-in `name`/`length` slots are `configurable:true`,
        // so a delete records the key in the closure deleted-key side table
        // (consulted by hasOwnProperty/getOwnProperty*/value reads);
        // user-attached props are dropped from the dynamic-prop table outright.
        if crate::closure::is_closure_ptr(obj as usize) {
            if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                // A plain (non-arrow, non-bound) function's `prototype` is a
                // non-configurable own property. `get_property_attrs` only knows
                // about it once #3655 has lazily registered a descriptor (on first
                // access), so a fresh `function(){}` whose prototype was never
                // read would otherwise report a successful delete. Reject it up
                // front. (test262 Proxy/deleteProperty/*-target-is-proxy exercise
                // `delete funcProxy.prototype` in strict mode.)
                if name == "prototype" {
                    let closure = obj as *const crate::closure::ClosureHeader;
                    if !crate::closure::closure_is_arrow(closure)
                        && !crate::closure::closure_is_bound_method(closure)
                    {
                        return 0;
                    }
                }
                // A non-configurable slot — e.g. a constructor's `prototype`,
                // which #3655 registers as `{configurable:false}` — can't be
                // deleted: leave it intact and report failure (strict mode
                // throws on the `false` return; sloppy mode no-ops).
                if let Some(attrs) = get_property_attrs(obj as usize, name) {
                    if !attrs.configurable() {
                        return 0;
                    }
                }
                crate::closure::closure_delete_own_dynamic_prop(obj as usize, name);
                crate::closure::closure_mark_key_deleted(obj as usize, name);
            }
            return 1;
        }
        // An accessor-ONLY property (defineProperty get/set with no data
        // slot) has no keys_array entry — the scan below would "succeed
        // vacuously" while leaving the descriptor in the side table, so
        // `delete obj[1]` left a ghost accessor behind (test262
        // map/15.4.4.19-8-b-8: a getter deletes a sibling accessor
        // mid-iteration and HasProperty must turn false).
        if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
            if get_accessor_descriptor(obj as usize, name).is_some() {
                if let Some(attrs) = get_property_attrs(obj as usize, name) {
                    if !attrs.configurable() {
                        return 0;
                    }
                }
                // Deleting an accessor from a class/Object prototype changes
                // method resolution for this key just like installing it.
                super::descriptor_state::disable_inline_guards_for_descriptor_target(
                    obj as usize,
                    name,
                );
                super::clear_accessor_descriptor(obj as usize, name);
                super::clear_property_attrs(obj as usize, name);
                // defineProperty may ALSO have planted a keys_array
                // placeholder entry for the key — fall through to the scan
                // below so hasOwnProperty / Object.keys stop seeing it.
            }
        }
        // A class-declaration prototype object: instance accessors (`get x()`)
        // live ONLY in the class vtable, so the scan below would "succeed
        // vacuously" while the accessor stayed visible to hasOwnProperty /
        // getOwnPropertyDescriptor. Record the key as deleted so those
        // reflective paths agree it is gone (test262 verifyProperty's
        // `configurable` check: `delete obj[name]` then assert the key absent —
        // class/definition/{getters,setters}-prop-desc).
        //
        // Instance METHODS are different: `install_class_decl_prototype_method_fields`
        // plants each one as a REAL keys_array data field on the prototype
        // object (writable:true, enumerable:false, configurable:true). Marking
        // the vtable key deleted is necessary but NOT sufficient — we must also
        // fall through to the keys scan to drop that real field, or
        // hasOwnProperty keeps finding the method via `own_key_present` and
        // verifyProperty fails "m descriptor should be configurable" (#5441,
        // ~760 generated language/{statements,expressions}/class tests).
        if let Some(cid) = super::class_registry::class_id_for_decl_prototype_object(obj as usize) {
            if let Some(name) = super::has_own_helpers::str_from_string_header(key) {
                if name != "constructor"
                    && (super::class_registry::class_own_accessor_ptrs(cid, name).is_some()
                        || super::native_module::class_has_own_method(cid, name)
                        || super::class_registry::lookup_own_prototype_method(cid, name).is_some())
                {
                    super::class_registry::class_mark_key_deleted(cid, name);
                    super::class_registry::invalidate_class_prototype_fast_guards_for_method(name);
                    crate::typed_feedback::invalidate_method_change(cid);
                    // Accessors have no keys_array entry, so the scan below is a
                    // vacuous success for them; methods DO, so fall through to
                    // remove it. Either way, don't early-return.
                }
            }
        }
        let keys = crate::object::object_keys_array(obj);
        if keys.is_null() {
            // No keys array means no fields to delete, but delete "succeeds" vacuously
            return 1;
        }

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        // #6759: shape-index + dense-slot scan (SSO-aware via the shared
        // helper, preserving #1781). The old per-element `js_array_get` walk
        // made every `delete` O(keys) full-accessor calls — measured as the
        // dominant residue (16.0 M of 90.8 M accessor calls) after the
        // [[Set]]/[[Get]] walks were fixed.
        let found_idx: Option<usize> =
            crate::object::keys_find_slot_by_key_ptr(keys, key_count as u32, key)
                .map(|i| i as usize);

        let i = match found_idx {
            Some(i) => i,
            None => return 1, // Not found — delete succeeds vacuously
        };
        let key_name = {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).ok()
        };
        if let Some(name) = key_name {
            if let Some(attrs) = get_property_attrs(obj as usize, name) {
                if !attrs.configurable() {
                    return 0;
                }
            }
            // A configurable data method on a class/Object prototype is about
            // to disappear. Retire only this name's direct-method guards.
            super::descriptor_state::disable_inline_guards_for_descriptor_target(
                obj as usize,
                name,
            );
        }

        // Proper delete: shift remaining keys + values down by one, then
        // shorten keys_array. Pre-fix this just set the value to
        // undefined and left the key in place, so `Object.keys`,
        // `Object.entries`, `for-in` etc. all still saw the deleted
        // property. Bun and Node remove the property entirely; we
        // match that.
        let field_count = crate::object::object_live_slot_count(obj);
        let alloc_limit = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
        let new_count = key_count - 1;

        // The clone below exists because one keys_array is shared by every
        // object that built this shape through a `transition_cache_lookup`
        // hit: mutating it in place would silently drop entries from siblings
        // that never deleted anything.
        //
        // Sharing is TRACKED. The caches stamp `GC_FLAG_SHAPE_SHARED` when
        // they publish an array (`transition_cache_insert`), and both the
        // ordinary `[[Set]]` growth path and `object_ops::keys_array` already
        // treat that bit as authoritative for exactly this decision. An array
        // without it has a single owner, so it can be compacted in place —
        // removing the last per-delete allocation on this path (a ~500-element
        // clone, 200k of them on `bench_populated_delete.ts`) and keeping the
        // array's ADDRESS, so the key index only needs its slots shifted.
        let keys_gc_header =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let keys_owned = (*keys_gc_header).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED == 0;
        // O(1) tombstone delete (flag-gated, #9020's Map pattern applied to
        // objects). An OWNED keys array can take a hole marker in place of
        // the deleted key: survivors keep their slots, so nothing shifts, no
        // layout rebuilds, and the live inline-slot bound is untouched. The
        // shape publish mints a fresh semantic generation, which is what
        // retires every cached (token, key) pair for this receiver — a
        // deleted key must stop hitting even though the array address and
        // every surviving slot are byte-identical, or a stale IC hit would
        // return the cleared slot instead of walking the prototype chain.
        // The read-plan epoch was already bumped at this function's entry.
        //
        // Tombstones are squeezed out when they reach half the slots (the
        // Map threshold), which amortizes compaction to O(1) per delete and
        // bounds the array at 2x its live size.
        if keys_owned && object_tombstone_deletes_enabled() {
            let holes = super::shapes::object_shape_hole_count(obj);
            let threshold_hit = key_count >= 16 && (holes + 1) * 2 > key_count as u32;
            if !threshold_hit {
                let successor = super::shapes::publish_object_shape_holes(obj, holes + 1);
                if successor != 0 {
                    let elements = (keys as *mut u8).add(std::mem::size_of::<crate::ArrayHeader>())
                        as *mut f64;
                    // Barriered stores, exactly the Map delete's idiom: the
                    // hole overwrites a key POINTER and the clear overwrites
                    // the value, so SATB marking must shade both children.
                    crate::gc::runtime_store_external_jsvalue_slot(
                        keys as usize,
                        elements.add(i) as usize,
                        crate::value::TAG_HOLE,
                    );
                    if i < alloc_limit {
                        let fields_ptr =
                            (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
                        crate::gc::runtime_store_jsvalue_slot(
                            obj as usize,
                            fields_ptr.add(i) as usize,
                            i,
                            crate::value::TAG_UNDEFINED,
                        );
                    } else {
                        overflow_set(obj as usize, i, crate::value::TAG_UNDEFINED);
                    }
                    return 1;
                }
                // Unstamped/unshaped receiver: fall through to the
                // compacting delete below, which needs no shape stamp.
            } else {
                // Threshold: squeeze every hole plus this key in one pass,
                // then continue through the ordinary compaction bookkeeping
                // is unnecessary — the squeeze does its own.
                squeeze_holes_and_delete(
                    obj,
                    keys,
                    i,
                    key_count,
                    alloc_limit,
                    field_count,
                    crate::object::reserved_slot_floor_for_class_id((*obj).class_id) as usize,
                );
                return 1;
            }
        }
        let index_migrated = if keys_owned {
            let elements =
                (keys as *mut u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *mut f64;
            // Overlapping ranges inside ONE allocation: `copy` (memmove).
            //
            // Unlike the clone arm below, this destination is the LIVE,
            // PUBLISHED array, so that arm's "unpublished, so no barrier" does
            // not carry over. No new referent is introduced -- every value
            // moved was already in this array one slot higher.
            // Same shape as `Array.prototype.splice`'s tail memmove
            // (`array/splice_slice.rs`), and safe for the same reason.
            if new_count > i {
                // GC_STORE_AUDIT(BARRIERED): `rebuild_array_layout_from_slots`
                // runs just below and, for an old-gen array, re-runs
                // `runtime_write_barrier_slot` over every slot; `length` is set
                // first so it covers exactly the compacted range.
                std::ptr::copy(elements.add(i + 1), elements.add(i), new_count - i);
            }
            (*keys).length = new_count as u32;
            super::rebuild_array_layout_from_slots(keys);
            // Re-publish the shape for the SAME array at its new key count.
            // Without this the object keeps a stamped ShapeId whose descriptor
            // still claims the pre-delete count, which the shape-facts audit
            // catches as "published ShapeId disagrees with authoritative
            // ObjectHeader facts". `publish_object_shape_from` versions a
            // same-pointer change internally, and `keys_changed` is false here
            // so the typed layout is preserved rather than marked unknown.
            set_object_keys_array(obj, keys as *mut crate::ArrayHeader);
            super::shapes::shape_index_shift_in_place(keys as usize, i as u32, key_count as u32)
        } else {
            let keys_cloned = crate::array::js_array_alloc(new_count.max(1) as u32 + 4);
            let src_elements =
                (keys as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
            let dst_elements =
                (keys_cloned as *mut u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *mut f64;
            // Copy keys [0..i) ++ [i+1..N) into [0..new_count) as two contiguous
            // runs. These were scalar element loops, which is O(resident keys) of
            // load/store pairs on a path that already allocates and rebuilds a
            // layout per delete — and `delete obj[k]` on a populated object is
            // perry's worst object-model gap against node (~200x on
            // `bench_populated_delete.ts`).
            //
            // GC_STORE_AUDIT(INIT): the destination is a freshly allocated, still
            // UNPUBLISHED keys array whose layout is rebuilt before it is
            // published — which is why the per-element writes carried no barrier
            // either. Source and destination are distinct allocations, so the
            // copies cannot overlap.
            if i > 0 {
                std::ptr::copy_nonoverlapping(src_elements, dst_elements, i);
            }
            if new_count > i {
                // GC_STORE_AUDIT(INIT): same unpublished destination as the run
                // above — freshly allocated keys array, layout rebuilt before
                // `set_object_keys_array` publishes it, distinct allocations.
                std::ptr::copy_nonoverlapping(
                    src_elements.add(i + 1),
                    dst_elements.add(i),
                    new_count - i,
                );
            }
            (*keys_cloned).length = new_count as u32;
            super::rebuild_array_layout_from_slots(keys_cloned);
            // Carry the key index onto the clone by shifting slots, instead of
            // letting the new address miss `indices` and re-hash every surviving
            // property name. On a 500-key object that rebuild ran on EVERY delete.
            // A wrong index can only cause a miss — `shape_slot_lookup` validates
            // the stored key against the requested bytes before returning a slot.
            let index_migrated = super::shapes::shape_index_migrate_after_delete(
                keys as usize,
                keys_cloned as usize,
                i as u32,
                key_count as u32,
                !keys_owned,
            );
            // `set_object_keys_array` publishes the cloned edge while preserving
            // the predecessor's semantic generation and object kind.
            set_object_keys_array(obj, keys_cloned);
            index_migrated
        };

        // 1) Shift values down: for slot j in i..new_count, copy slot j+1
        //    into slot j. Inline reads/writes for j < alloc_limit;
        //    overflow_get/set otherwise.
        for j in i..new_count {
            // Read through the index path, which resolves inline-vs-overflow with the
            // CURRENT (pre-decrement) `field_count` — the same boundary the value was
            // written under. Reading by NAME here would invoke a getter and store its
            // result as a data property, silently collapsing accessors (Next's module
            // exports are `Object.defineProperty(..., {get})`).
            // `js_object_get_field` resolves the live-slot bound itself, and
            // that bound is a shape-table probe — so shifting N values paid N
            // descriptor lookups per delete. This loop already holds the same
            // bound in `field_count`, and nothing in it changes the receiver's
            // shape, so pass it in. `object_field_at_with_live` exists for
            // exactly this (#8122) and is otherwise the identical body,
            // including the inline-vs-overflow split.
            let next = crate::object::field_get_set::object_field_at_with_live(
                obj,
                (j + 1) as u32,
                field_count,
            );
            // Inline write if target slot < alloc_limit, else overflow.
            if j < alloc_limit {
                let fields_ptr =
                    (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
                let slot = fields_ptr.add(j);
                crate::gc::runtime_store_jsvalue_slot(obj as usize, slot as usize, j, next.bits());
            } else {
                overflow_set(obj as usize, j, next.bits());
            }
        }
        // Clear the now-tail slot so reads past keys_array.length see undefined.
        if new_count < alloc_limit {
            let fields_ptr =
                (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
            let slot = fields_ptr.add(new_count);
            crate::gc::runtime_store_jsvalue_slot(
                obj as usize,
                slot as usize,
                new_count,
                crate::value::TAG_UNDEFINED,
            );
        } else {
            overflow_set(obj as usize, new_count, crate::value::TAG_UNDEFINED);
        }

        // 2) (Keys already shifted into the cloned keys_array above —
        //    we built the new keys directly with the deleted entry
        //    omitted, so no in-place shift is needed.)

        // 3) `field_count` is the number of properties resident in the INLINE
        //    slots — every reader treats `field_index >= field_count` as living in
        //    the overflow map. It is NOT the property count: an object with 9
        //    properties and 8 inline slots carries `field_count == 8`, with the 9th
        //    spilled to overflow.
        //
        //    Decrementing it by one was therefore wrong. Deleting one property from
        //    that 9-property object leaves 8 survivors — all of which now FIT
        //    inline, so `field_count` must become 8. The old `field_count - 1 = 7`
        //    pushed the last survivor's index (7) at or past the boundary, so
        //    reading it went to the overflow map, found nothing, and returned
        //    `undefined`: the key stayed enumerable while its value vanished.
        //
        //    After the rebuild above, the survivors occupy slots `0..new_count`,
        //    inline up to the allocation's capacity. That is exactly
        //    `min(new_count, alloc_limit)`.
        set_object_live_slot_count(obj, std::cmp::min(new_count, alloc_limit) as u32);

        // 4) Drop the (post-compaction) keys array's slot-index accelerator —
        //    slots past `i` have shifted, so any map is stale. Descriptors are
        //    not eagerly deleted because a sibling may still name one; exact
        //    new facts are published below and weak post-trace pruning retires
        //    dead historical descriptors.
        // ...unless the migration above already shifted it to match the
        // compacted array, in which case it is CURRENT, not stale, and
        // dropping it would throw away the rebuild this is meant to avoid —
        // the next lookup would re-hash every surviving key name.
        if !index_migrated {
            crate::object::shapes::shape_drop(crate::object::object_keys_array(obj));
        }
        1
    }
}

/// True when `value` is a heap object the delete path may dereference (object,
/// array, function, string, class-ref, proxy, …) — i.e. a NaN-boxed pointer.
/// Primitive numbers/booleans are NOT pointers; unboxing their bits as an
/// `ObjectHeader*` yields a garbage address that crashes when the GC/kind header
/// at `[ptr-8]` is read.
#[inline]
fn delete_receiver_is_pointer(obj_value: f64) -> bool {
    crate::value::JSValue::from_bits(obj_value.to_bits()).is_pointer()
}

fn delete_class_prototype_key(class_id: u32, name: &str) -> i32 {
    let has_own = name == "constructor"
        || super::native_module::class_has_own_method(class_id, name)
        || super::class_registry::class_own_accessor_ptrs(class_id, name).is_some()
        || super::class_registry::lookup_own_prototype_method(class_id, name).is_some();
    if !has_own {
        return 1;
    }
    super::class_registry::class_mark_key_deleted(class_id, name);
    super::class_registry::invalidate_class_prototype_fast_guards_for_method(name);
    crate::typed_feedback::invalidate_method_change(class_id);
    1
}

/// `delete prim.field` (static key): once RequireObjectCoercible has rejected
/// null/undefined, a primitive receiver (number/boolean/…) has no deletable own
/// property, so `delete` is a no-op that evaluates to `true` (spec ToObject of a
/// primitive produces a throwaway wrapper). Takes the RAW NaN-boxed receiver so
/// the pointer/primitive tag survives; the previous codegen unboxed primitives
/// to a garbage `ObjectHeader*` → EXC_BAD_ACCESS in `js_object_delete_field`.
#[no_mangle]
pub extern "C" fn js_object_delete_field_value(
    obj_value: f64,
    key: *const crate::StringHeader,
) -> i32 {
    if let Some(class_id) = super::class_prototype_ref_id(obj_value) {
        if key.is_null() {
            return 1;
        }
        return unsafe {
            super::has_own_helpers::str_from_string_header(key)
                .map(|name| delete_class_prototype_key(class_id, name))
                .unwrap_or(1)
        };
    }
    // A class reference (`delete C.m` for a `static m()`) is INT32-tagged, so
    // `is_pointer` is false and the guard below would no-op it. But a static
    // member delete must still unregister the method/field. `js_object_delete_field`
    // already treats a sub-0x10000 "pointer" as a class id, so forward the id
    // there. (#5579: #5490 rerouted `delete` through this value-form wrapper,
    // whose primitive guard silently dropped class-ref receivers → static
    // `delete C.m` became a vacuous no-op and verifyProperty's configurable
    // check failed "m descriptor should be configurable".)
    if let Some(class_id) = super::native_module::class_ref_id(obj_value) {
        return js_object_delete_field(class_id as usize as *mut ObjectHeader, key);
    }
    if !delete_receiver_is_pointer(obj_value) {
        return 1;
    }
    let obj = crate::value::js_nanbox_get_pointer(obj_value) as *mut ObjectHeader;
    js_object_delete_field(obj, key)
}

/// `delete prim[key]` (dynamic key) — same primitive-receiver no-op guard as
/// `js_object_delete_field_value`, delegating real objects to the dynamic path.
#[no_mangle]
pub extern "C" fn js_object_delete_dynamic_value(obj_value: f64, key: f64) -> i32 {
    if let Some(class_id) = super::class_prototype_ref_id(obj_value) {
        return unsafe {
            super::native_module::metadata_key_to_string(key)
                .map(|name| delete_class_prototype_key(class_id, &name))
                .unwrap_or(1)
        };
    }
    // Class-ref receiver (`delete C["m"]`): see `js_object_delete_field_value`.
    if let Some(class_id) = super::native_module::class_ref_id(obj_value) {
        return js_object_delete_dynamic(class_id as usize as *mut ObjectHeader, key);
    }
    if !delete_receiver_is_pointer(obj_value) {
        return 1;
    }
    let obj = crate::value::js_nanbox_get_pointer(obj_value) as *mut ObjectHeader;
    js_object_delete_dynamic(obj, key)
}

/// Delete a field from an object using a dynamic key (could be string or number index)
/// Returns 1 if successful, 0 otherwise
#[no_mangle]
pub extern "C" fn js_object_delete_dynamic(obj: *mut ObjectHeader, key: f64) -> i32 {
    if let Some((_, elements)) = unsafe { crate::array::subclass_elements::backed(obj as usize) } {
        if let Some(elements_key) = crate::array::subclass_elements::key_of_value(key) {
            return unsafe { crate::array::subclass_elements::delete_key(elements, elements_key) };
        }
    }
    // Proxy receiver (small registered id) — route through the proxy
    // `deleteProperty` trap before any key coercion that would deref the fake
    // pointer. Handles symbol keys too (the string path also funnels into
    // `js_object_delete_field`, which has its own guard).
    {
        let addr = obj as u64;
        if crate::value::addr_class::is_proxy_id_band(addr as usize) {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let r = crate::proxy::js_proxy_delete(boxed, key);
                return if crate::value::js_is_truthy(r) != 0 {
                    1
                } else {
                    0
                };
            }
        }
    }
    let key_val = JSValue::from_bits(key.to_bits());

    // If the key is a string, use js_object_delete_field. #1781: accept
    // inline SSO short keys — `delete obj["abc"]` for a <=5-char key arrives
    // as a SHORT_STRING_TAG value that is_string() rejects, so the delete
    // silently no-op'd (fell through to "succeeds vacuously"). Materialize
    // the key to a heap header so js_object_delete_field can match it.
    if key_val.is_any_string() {
        let key_str =
            crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
        return js_object_delete_field(obj, key_str);
    }

    // #6935: the string-key case returned above, so `key` here is a number, a
    // BigInt, a boolean, `null`/`undefined` — or an OBJECT, whose
    // `Symbol.toPrimitive` / `toString` / `valueOf` runs user JS. Either way
    // `js_to_property_key` allocates and can trigger a GC that **evacuates**
    // the receiver, and `obj` is a bare Rust local across it. Root it and read
    // it back through the handle for both the symbol and string delete arms.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let property_key = unsafe { js_to_property_key(key) };
    let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        // Symbol-keyed delete (`delete obj[Symbol.iterator]`). Previously this
        // fell through to the vacuous `return 1`, so the delete *reported*
        // success while leaving the property in place — `verifyProperty`'s
        // `isConfigurable` (delete-then-hasOwn) then saw the property survive
        // and flagged a configurable symbol property as non-configurable
        // (Test262 `Map.prototype/Symbol.iterator.js`). Route to the symbol
        // property table delete, which honors the configurable attribute.
        let obj_f64 = crate::value::js_nanbox_pointer(obj as i64);
        return unsafe { crate::symbol::js_object_delete_symbol_property(obj_f64, property_key) };
    }
    let property_key_handle = scope.root_nanbox_f64(property_key);
    let key_str = crate::value::js_jsvalue_to_string(property_key_handle.get_nanbox_f64());
    if !key_str.is_null() {
        return js_object_delete_field(
            obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
            key_str as *const crate::StringHeader,
        );
    }

    // For other types, delete succeeds vacuously
    1
}

/// Create a rest object from destructuring: copies all properties from src except excluded keys.
/// exclude_keys is an array of NaN-boxed string pointers (the explicitly destructured keys).
/// Returns a pointer to a new object with the remaining key-value pairs.
#[no_mangle]
pub extern "C" fn js_object_rest(
    src: *const ObjectHeader,
    exclude_keys: *const ArrayHeader,
) -> *mut ObjectHeader {
    if src.is_null() {
        return js_object_alloc(0, 0);
    }
    unsafe {
        let keys = crate::object::object_keys_array(src);
        if keys.is_null() {
            return js_object_alloc(0, 0);
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        let exclude_count = if exclude_keys.is_null() {
            0
        } else {
            crate::array::js_array_length(exclude_keys) as usize
        };

        // Collect indices of keys to include (not in exclude list and not undefined/deleted).
        // #1781: SSO-aware — the pre-fix `is_string()` on the source
        // key dropped ≤5-byte SSO keys from `rest`; the exclude-loop's
        // `is_string()` similarly missed inline-SSO exclude entries,
        // so a `{a, ...rest}` pattern silently kept `a` in `rest` when
        // both the source key and the exclude key were SSO.
        let mut include_indices: Vec<usize> = Vec::new();
        let mut src_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            let key_bytes = match crate::string::js_string_key_bytes(key_val, &mut src_buf) {
                Some(b) => b.to_vec(),
                None => continue,
            };

            // Check if field was deleted
            let field_val = js_object_get_field(src, i as u32);
            if field_val.is_undefined() {
                continue;
            }

            // Check if this key is in the exclude list
            let mut excluded = false;
            for j in 0..exclude_count {
                let ex_val = crate::array::js_array_get(exclude_keys, j as u32);
                if crate::string::js_string_key_matches_bytes(ex_val, &key_bytes) {
                    excluded = true;
                    break;
                }
            }
            if !excluded {
                include_indices.push(i);
            }
        }

        // Allocate new object with the right number of fields
        let rest_count = include_indices.len() as u32;
        let rest_obj = js_object_alloc(0, rest_count);

        // Create keys array for the rest object
        let rest_keys = crate::array::js_array_alloc_with_length(rest_count);
        set_object_keys_array(rest_obj, rest_keys);

        // Copy included key-value pairs
        for (new_idx, &src_idx) in include_indices.iter().enumerate() {
            let key_val = crate::array::js_array_get(keys, src_idx as u32);
            crate::array::js_array_set(rest_keys, new_idx as u32, key_val);

            let field_val = js_object_get_field(src, src_idx as u32);
            js_object_set_field(rest_obj, new_idx as u32, field_val);
        }

        rest_obj
    }
}

#[cfg(test)]
mod shape_transition_tests_6759 {
    //! #6759 C3: what a `delete` does to an object's SHAPE IDENTITY, pinned for
    //! both object representations — because rung 1 made them AGREE, and that
    //! agreement is the entry gate for the header shrink (#7916).
    //!
    //! `perry-codegen`'s `class_field_inline_guard` speculates that a receiver's
    //! packed slot layout is its class's canonical one. `delete` breaks that
    //! (slots after the deleted key shift down one) while PRESERVING
    //! `class_id`, so the guard compares the live `keys_array` POINTER against
    //! the class's `@perry_class_keys_*` token. Replacing that pointer compare
    //! with a one-word ShapeId compare — which is what makes the header shrink
    //! a load *removal* instead of a load-for-probe trade — requires a class
    //! instance to HAVE a shape word.
    //!
    //! Before rung 1 it did not: `parent_class_id` was the shape word only when
    //! `class_id == 0`, so the sibling test below asserted the ABSENCE of one
    //! and named its own replacement. Rung 1 removed the `class_id` gate, so
    //! both representations now mint a fresh id across a delete and these tests
    //! state the same property twice, once per representation.
    //!
    //! Rung 1 is deliberately runtime-only: the guard has NOT switched to the
    //! id compare (that is rung 3), and codegen's inline `new C()` still writes
    //! a constant `parent_cid`, so an instance is stamped LAZILY at its first
    //! by-name resolve rather than at birth (rung 2). The tests below therefore
    //! resolve a field before reading the stamp — a fresh instance legitimately
    //! reads as unstamped.
    use super::*;
    use crate::object::shapes::is_shape_id;

    fn key(name: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    /// A plain object's `delete` mints a new authoritative ShapeId eagerly. The
    /// stale slot index is dropped, the stamp is cleared, then the compacted
    /// facts install a genuinely fresh id rather than reviving the old one.
    /// Ids are never reused, so "different" is the whole property.
    #[test]
    fn delete_mints_a_fresh_shape_id_for_a_plain_object() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 8);
            for name in ["del6759_a", "del6759_b", "del6759_c"] {
                crate::object::js_object_set_field_by_name(obj, key(name), 1.0);
            }
            let _ = crate::object::js_object_get_field_by_name(obj, key("del6759_b"));
            let before = (*obj).parent_class_id;
            assert!(
                is_shape_id(before),
                "fixture is vacuous — no shape stamp to transition (got {before:#x})"
            );

            assert_eq!(js_object_delete_field(obj, key("del6759_a")), 1);

            // The compacted descriptor is installed before delete returns.
            let after = (*obj).parent_class_id;
            assert!(
                is_shape_id(after),
                "no shape id re-minted after the delete (got {after:#x})"
            );
            assert_ne!(
                after, before,
                "the delete re-used the pre-delete ShapeId — a shape-id compare \
                 would accept a compacted object as its own class's shape"
            );
            let descriptor = crate::object::shapes::shape_descriptor_by_id(after)
                .expect("delete must publish a by-id descriptor");
            assert_eq!(
                descriptor.keys,
                crate::object::object_keys_array(obj) as u64
            );
            assert_eq!(descriptor.logical_key_count, 2);
            assert_eq!(
                descriptor.live_inline_slot_count,
                crate::object::object_live_slot_count(obj)
            );
        }
    }

    /// #6759 C3 rung 1 — the replacement the pre-rung-1 test named.
    ///
    /// This is the assertion `delete_leaves_a_class_instance_with_no_shape_word_to_transition`
    /// asked to be replaced by. A class instance now HAS a shape word, so a
    /// `delete` on it transitions the same way a plain object's does: the
    /// compaction clears the stamp, then installs a genuinely fresh id before
    /// returning (ids are never reused) rather than reviving the class's
    /// canonical one.
    ///
    /// The old test's other half — that `class_id` is preserved and the keys
    /// POINTER moves — is kept below, because both are still true and both are
    /// still what `class_field_inline_guard` compares until rung 3.
    #[test]
    fn delete_mints_a_fresh_shape_id_for_a_class_instance() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_6760;
        const PARENT: u32 = 0x0C3C_6761;
        let packed = b"del6759_x\0del6759_y\0del6759_z";
        unsafe {
            let obj = crate::object::js_object_alloc_class_with_keys(
                CID,
                PARENT,
                3,
                packed.as_ptr(),
                packed.len() as u32,
            );
            for (i, v) in [10.0f64, 20.0, 30.0].iter().enumerate() {
                js_object_set_field(obj, i as u32, JSValue::from_bits(v.to_bits()));
            }
            let keys_before = crate::object::object_keys_array(obj);
            assert_eq!((*obj).class_id, CID, "test premise: a class instance");
            let before = (*obj).parent_class_id;
            assert!(
                is_shape_id(before),
                "test premise: a class instance is stamped AT BIRTH (got \
                 {before:#x}). Rung 2 (#8009 for the compiled path, and the \
                 runtime allocators alongside it) exists because a LAZY stamp \
                 splits the shape's population — see \
                 `shapes::birth_stamp_object_shape`"
            );
            // A by-name resolve must not change it: the birth stamp is already
            // the id every later resolve would have minted.
            let _ = crate::object::js_object_get_field_by_name(obj, key("del6759_y"));
            assert_eq!(
                (*obj).parent_class_id,
                before,
                "a resolve re-stamped an already-stamped instance with a \
                 DIFFERENT id — every site holding the birth token would miss"
            );

            assert_eq!(js_object_delete_field(obj, key("del6759_x")), 1);

            // The compaction really happened: `z` moved from slot 2 to slot 1.
            assert_eq!(
                f64::from_bits(js_object_get_field(obj, 1).bits()),
                30.0,
                "test premise: the delete did not compact the slots"
            );
            // The compacted descriptor is installed before delete returns.
            let after = (*obj).parent_class_id;
            assert!(
                is_shape_id(after),
                "no shape id re-minted after the delete (got {after:#x})"
            );
            assert_ne!(
                after, before,
                "the delete re-used the pre-delete ShapeId — a shape-id compare \
                 would accept a compacted class instance as its own class's shape"
            );
            let descriptor = crate::object::shapes::shape_descriptor_by_id(after)
                .expect("class delete must publish a by-id descriptor");
            assert_eq!(
                descriptor.keys,
                crate::object::object_keys_array(obj) as u64
            );
            assert_eq!(descriptor.logical_key_count, 2);
            assert_eq!(
                descriptor.live_inline_slot_count,
                crate::object::object_live_slot_count(obj)
            );

            // Still true, and still what the guard compares until rung 3.
            assert_ne!(
                crate::object::object_keys_array(obj),
                keys_before,
                "the keys pointer is the guard's compaction evidence and it did not change"
            );
            assert_eq!(
                (*obj).class_id,
                CID,
                "class_id must survive a delete — it is the vtable/instanceof identity"
            );
        }
    }

    /// Two pristine instances of the same class share ONE ShapeId (their
    /// canonical keys array is shared), and a delete on one moves only that
    /// one. This is the property a rung-3 id compare rests on, stated
    /// separately from the mint test so a regression names which half broke.
    #[test]
    fn class_siblings_share_one_shape_id_until_one_is_deleted_from() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_6762;
        let packed = b"sib6759_a\0sib6759_b\0sib6759_c";
        unsafe {
            let mk = || {
                crate::object::js_object_alloc_class_with_keys(
                    CID,
                    0,
                    3,
                    packed.as_ptr(),
                    packed.len() as u32,
                )
            };
            let a = mk();
            let b = mk();
            let _ = crate::object::js_object_get_field_by_name(a, key("sib6759_b"));
            let _ = crate::object::js_object_get_field_by_name(b, key("sib6759_b"));
            let id_a = (*a).parent_class_id;
            let id_b = (*b).parent_class_id;
            assert!(
                is_shape_id(id_a) && is_shape_id(id_b),
                "both must be stamped"
            );
            assert_eq!(
                id_a, id_b,
                "same-class siblings must share one ShapeId — otherwise an \
                 id-comparing PIC is monomorphic-per-OBJECT and never hits"
            );

            assert_eq!(js_object_delete_field(a, key("sib6759_a")), 1);
            let _ = crate::object::js_object_get_field_by_name(a, key("sib6759_c"));
            assert_ne!(
                (*a).parent_class_id,
                id_b,
                "the compacted instance kept its siblings' ShapeId"
            );
            assert_eq!(
                (*b).parent_class_id,
                id_b,
                "the untouched sibling's stamp moved — a delete is not a \
                 class-wide shape transition"
            );
        }
    }

    /// #6759 C3 rung 1 risk check: stamping OVERWRITES the header's
    /// `parent_class_id`, so the class-parent chain must be served entirely by
    /// the class-id-keyed registry (rung 0 / #7981 removed the last header
    /// reader). A 3-level chain must still resolve after every level's header
    /// word has been clobbered by a stamp.
    #[test]
    fn a_stamped_class_instance_still_resolves_a_three_level_parent_chain() {
        let _lock = crate::gc::global_side_table_test_lock();
        const BASE: u32 = 0x0C3C_6770;
        const MID: u32 = 0x0C3C_6771;
        const LEAF: u32 = 0x0C3C_6772;
        let packed = b"chain6759_p\0chain6759_q";
        unsafe {
            let leaf = crate::object::js_object_alloc_class_with_keys(
                LEAF,
                MID,
                2,
                packed.as_ptr(),
                packed.len() as u32,
            );
            // The registry edges are registered by the allocator (codegen's
            // inline `new C()` path registers them from the module-init
            // prelude instead); MID→BASE has no instance here, so register it
            // the way that prelude does.
            crate::object::register_class(MID, BASE);
            // Rung 2: the word is a ShapeId from BIRTH, so the parent edge is
            // already only in the registry before anything below runs. That
            // makes this test stronger than when the word still started as
            // inheritance data — there is no window in which it was correct.
            assert!(
                is_shape_id((*leaf).parent_class_id),
                "test premise: the newborn's header word was not clobbered by a \
                 birth stamp, so the chain is not actually being stressed"
            );

            let boxed = crate::value::js_nanbox_pointer(leaf as i64);
            let truthy = |v: f64| crate::value::js_is_truthy(v) != 0;
            assert!(truthy(crate::object::js_instanceof(boxed, LEAF)));
            assert!(truthy(crate::object::js_instanceof(boxed, MID)));
            assert!(truthy(crate::object::js_instanceof(boxed, BASE)));

            // …and it stays clobbered across a resolve.
            let _ = crate::object::js_object_get_field_by_name(leaf, key("chain6759_p"));
            assert!(
                is_shape_id((*leaf).parent_class_id),
                "test premise: the word stopped being a stamp, so nothing is being tested"
            );

            assert!(
                truthy(crate::object::js_instanceof(boxed, LEAF)),
                "own class lost after stamping"
            );
            assert!(
                truthy(crate::object::js_instanceof(boxed, MID)),
                "direct parent lost after the header word was overwritten — the \
                 parent edge is not coming from the registry"
            );
            assert!(
                truthy(crate::object::js_instanceof(boxed, BASE)),
                "grandparent lost after the header word was overwritten"
            );
        }
    }
}

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    /// #1781: `delete obj["id"]` for a key <= 5 bytes — the dynamic key
    /// arrives as an inline SSO value that `is_string()` (STRING_TAG-only)
    /// rejected, so the delete silently no-op'd (fell through to "succeeds
    /// vacuously") and the property stayed put.
    #[test]
    fn delete_dynamic_removes_property_via_sso_key() {
        {
            let obj = crate::object::js_object_alloc(0, 0);
            let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
            crate::object::js_object_set_field_by_name(obj, key, 42.0);

            let obj_box = crate::value::js_nanbox_pointer(obj as i64);
            let sso = crate::value::JSValue::try_short_string(b"id").unwrap();
            assert!(sso.is_short_string());
            // present before delete
            assert_ne!(
                crate::value::js_is_truthy(crate::object::js_object_has_property(
                    obj_box,
                    f64::from_bits(sso.bits())
                )),
                0
            );

            let ok = js_object_delete_dynamic(obj, f64::from_bits(sso.bits()));
            assert_eq!(ok, 1, "delete should report success");

            // gone after delete
            assert_eq!(
                crate::value::js_is_truthy(crate::object::js_object_has_property(
                    obj_box,
                    f64::from_bits(sso.bits())
                )),
                0,
                "SSO key should be removed after delete"
            );
        }
    }
}

/// Gate for O(1) tombstone deletes (`PERRY_OBJECT_TOMBSTONES=1`). Default OFF
/// while the walker audit and differentials bake; the sibling Map tombstones
/// (#9020) shipped default-on after the same sequence.
fn object_tombstone_deletes_enabled() -> bool {
    // Test override first: the OnceLock latches at the FIRST delete anywhere
    // in the test process, which is long before a tombstone test's own
    // `set_var` — so tests opt in through this cell instead of the env.
    #[cfg(test)]
    if let Some(forced) = TOMBSTONE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return forced;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        // Default ON (#9029 shipped the mechanism flag-gated; the walker
        // audit and churn-bound tests are the default-on prerequisites).
        // `PERRY_OBJECT_TOMBSTONES=0` is the kill switch, mirroring the
        // moving-scavenge rollout's `PERRY_GC_MOVING_LOOP_POLLS=0` pattern.
        !matches!(
            std::env::var("PERRY_OBJECT_TOMBSTONES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

#[cfg(test)]
thread_local! {
    static TOMBSTONE_TEST_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// Force the tombstone-delete flag for the CURRENT THREAD's asserts,
/// bypassing the env-latched OnceLock. Pass `None` to restore env behavior;
/// callers must do so before returning (tests share threads).
#[cfg(test)]
pub(crate) fn test_set_tombstone_deletes(forced: Option<bool>) {
    TOMBSTONE_TEST_OVERRIDE.with(|cell| cell.set(forced));
}

/// Threshold compaction for a tombstoned keys array: squeeze every hole AND
/// the key at `delete_slot` out in one overlap-safe pass, values moved to
/// match, then republish layout, live bound and shape. The cost equals what
/// ONE pre-tombstone delete paid, amortized over the deletes that created
/// the holes — `compact_map_entries`' argument, applied to objects.
///
/// # Safety
/// `obj` live and owned `keys` as its current keys array; `delete_slot <
/// key_count`; caller already bumped the read-plan epoch.
unsafe fn squeeze_holes_and_delete(
    obj: *mut ObjectHeader,
    keys: *const crate::ArrayHeader,
    delete_slot: usize,
    key_count: usize,
    alloc_limit: usize,
    field_count: u32,
    // #9019: a reserved-layout receiver's leading `floor` slots are
    // STRUCTURAL tombstones guarding its raw internal fields — squeezing
    // them would slide user keys back under the floor and re-open the
    // field-0 alias. They are exempt from compaction; only churn holes at
    // or past the floor are squeezed. (`delete_slot` is always >= floor
    // there: the reserved slots hold no key a delete could match.)
    reserved_floor: usize,
) {
    let keys = keys as *mut crate::ArrayHeader;
    let elements = (keys as *mut u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *mut f64;
    let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    let floor = reserved_floor.min(key_count);
    let mut out = floor;
    for s in floor..key_count {
        let kv = std::ptr::read(elements.add(s));
        if s == delete_slot || kv.to_bits() == crate::value::TAG_HOLE {
            continue;
        }
        if out != s {
            // Keys move DOWN within one buffer (out < s always) — same
            // overlap argument as `compact_map_entries`.
            // GC_STORE_AUDIT(EXTERNAL_BARRIERED): the dirty-span barrier after
            // this loop covers every surviving key slot written here, exactly
            // as compact_map_entries' squeeze is audited.
            std::ptr::write(elements.add(out), kv);
            // Value follows its key. Read through the index path against the
            // PRE-squeeze bound (the same boundary it was written under),
            // then store through the barriered inline/overflow split.
            let v =
                crate::object::field_get_set::object_field_at_with_live(obj, s as u32, field_count);
            if out < alloc_limit {
                crate::gc::runtime_store_jsvalue_slot(
                    obj as usize,
                    fields_ptr.add(out) as usize,
                    out,
                    v.bits(),
                );
            } else {
                overflow_set(obj as usize, out, v.bits());
            }
        }
        out += 1;
    }
    (*keys).length = out as u32;
    if out > 0 {
        // GC_STORE_AUDIT(EXTERNAL_BARRIERED): dirty-span barrier over the
        // compacted key slots, mirroring compact_map_entries.
        crate::gc::runtime_write_barrier_external_slot_span(keys as usize, elements as usize, out);
    }
    super::rebuild_array_layout_from_slots(keys);
    set_object_live_slot_count(obj, std::cmp::min(out, alloc_limit) as u32);
    // Slots moved: the per-array key index and any stale descriptors for the
    // pre-squeeze states are wrong now. Drop the index (rebuilt on demand)
    // and publish the squeezed shape at exactly the surviving hole count —
    // zero for an ordinary receiver, the structural reserved floor (#9019)
    // for an iterator-family one.
    crate::object::shapes::shape_drop(keys);
    super::shapes::publish_object_shape_holes(obj, floor as u32);
}
