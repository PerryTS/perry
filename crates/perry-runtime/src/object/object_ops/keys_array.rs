//! keys_array maintenance helpers shared by the descriptor-define paths:
//! `ensure_key_in_keys_array`, `install_builtin_getter`, `own_key_present`.
use super::*;

/// Ensure a key appears in the object's keys_array. Used by `Object.defineProperty`
/// so the property is enumerable-filterable and discoverable by `getOwnPropertyNames`
/// even when the value is undefined or the property is an accessor (no underlying slot).
/// Claim a keys slot for `key` WITHOUT writing a value: an accessor install, a
/// built-in getter, a generic descriptor that only adjusts attributes.
///
/// Such a claim may not adopt or publish a transition edge whose slot lives in
/// the overflow region, because the overflow entry the edge implies is never
/// created — see [`ensure_key_in_keys_array_inner`].
pub(crate) unsafe fn ensure_key_in_keys_array(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) {
    ensure_key_in_keys_array_inner(obj, key, false)
}

/// [`ensure_key_in_keys_array`] for a claim the caller will follow with a value
/// store — `define_property_force_store_value`, the data-descriptor path.
///
/// #10868 lever (iii). `INLINE_SLOT_FLOOR` is 2 and #7916 documents it as a
/// pure FOOTPRINT dial ("a growth-headroom dial for objects that gain
/// properties by name after birth"), so a two-field literal allocates exactly
/// two slots and the first key added after birth lands in overflow. The
/// publication and adoption gates below both refused an overflow-located edge,
/// so every `Object.defineProperty` receiver forked onto a private keys array
/// and re-minted the whole chain: measured at 3 ids per object on `accd` and
/// 4 on `acc`, linear and unbounded, with the transition cache reporting
/// lookups and **zero** inserts.
///
/// The `[[Set]]` tail already publishes an overflow-located edge with no such
/// gate (`field_set_by_name/tail.rs`, the overflow arm), which is why the
/// `pool` control — the same objects grown by name instead — mints a constant
/// 4,534 ids however much work it does. This is the missing wiring, not a
/// missing capability.
///
/// The refusal was correct for a keys-only claim and over-broad for a data
/// one, which is exactly the distinction this split draws.
pub(crate) unsafe fn ensure_key_in_keys_array_for_value(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) {
    ensure_key_in_keys_array_inner(obj, key, true)
}

/// `refresh_define_property_roots!` re-reads BOTH roots at every allocation
/// boundary and some paths read only one of them before the next boundary, so
/// the dead writes are deliberate: making the refresh conditional would make
/// the GC idiom conditional. (The attribute sat on the pre-split entry point.)
#[allow(unused_assignments)]
unsafe fn ensure_key_in_keys_array_inner(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    writes_value: bool,
) {
    if obj.is_null() || (obj as usize) < 0x10000 || key.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    let mut key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    macro_rules! refresh_define_property_roots {
        () => {{
            obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
        }};
    }
    // If no keys array exists, create one with this key.
    let mut keys = crate::object::object_keys_array(obj);
    if keys.is_null() {
        // #9019: a reserved-layout iterator receiver seeds its floor of
        // tombstones first, so a defineProperty key (this arm also serves
        // accessor installs, which claim a keys slot with no data write)
        // can never take a raw internal field's index. The seeded receiver
        // then falls through to the ordinary existing-keys append below.
        if crate::object::reserved_slot_floor_for_class_id((*obj).class_id) != 0 {
            let seeded = crate::object::ensure_reserved_floor_keys(obj);
            refresh_define_property_roots!();
            keys = crate::object::object_keys_array(obj);
            if !seeded && keys.is_null() {
                // Seed failed (allocation refused): drop the key claim
                // rather than let it take a raw internal field's index.
                return;
            }
        } else {
            // #10287: the FIRST key is the case that matters for a
            // function-constructed receiver (zod defines `_zod` before the
            // object has any key at all), and the `[[Set]]` tail already
            // learns these keyless→one-key edges. Try the shared edge before
            // minting a private array, and teach it otherwise.
            let transition_eligible_first = define_append_transition_eligible(obj, keys);
            let prev_shape_id = if transition_eligible_first {
                super::super::shapes::object_shape_stamp(obj)
            } else {
                0
            };
            let interned = if prev_shape_id != 0 {
                let interned = intern_define_key(&scope, key);
                refresh_define_property_roots!();
                interned
            } else {
                None
            };
            if let Some(handle) = interned.as_ref() {
                let probe = handle.with_const_ptr::<crate::StringHeader, _>(|interned_key| {
                    super::super::transition_cache_lookup(prev_shape_id, interned_key)
                });
                if let Some((next_keys, slot_idx, target_shape_id)) = probe {
                    let live = crate::object::object_live_slot_count(obj);
                    let alloc_limit = std::cmp::max(live, crate::object::INLINE_SLOT_FLOOR as u32);
                    if next_keys != 0
                        && (writes_value || slot_idx < alloc_limit)
                        && cached_target_fits(target_shape_id, alloc_limit)
                    {
                        if !super::super::shapes::install_cached_object_shape_transition(
                            obj,
                            prev_shape_id,
                            target_shape_id,
                            next_keys as *mut ArrayHeader,
                        ) {
                            set_object_keys_array(obj, next_keys as *mut ArrayHeader);
                        }
                        // Only an INLINE slot advances the live bound; see the
                        // same guard in the existing-keys arm below.
                        if slot_idx >= live && slot_idx < alloc_limit {
                            set_object_live_slot_count(obj, slot_idx + 1);
                        }
                        return;
                    }
                }
            }
            let new_keys = crate::array::js_array_alloc(4);
            refresh_define_property_roots!();
            let new_keys =
                crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
            refresh_define_property_roots!();
            set_object_keys_array(obj, new_keys);
            if crate::object::object_live_slot_count(obj) == 0 {
                set_object_live_slot_count(obj, 1);
            }
            if let Some(handle) = interned.as_ref() {
                let target_shape_id = super::super::shapes::object_shape_stamp(obj);
                let published_keys = crate::object::object_keys_array(obj);
                if target_shape_id != 0
                    && target_shape_id != prev_shape_id
                    && !published_keys.is_null()
                {
                    handle.with_const_ptr::<crate::StringHeader, _>(|interned_key| {
                        super::super::transition_cache_insert(
                            std::ptr::null(),
                            prev_shape_id,
                            interned_key,
                            published_keys as usize,
                            0,
                            target_shape_id,
                        )
                    });
                }
            }
            return;
        }
    }
    let keys = keys;
    // Validate keys array pointer. The bare high-bits/low-address checks let
    // through values that are non-null and tag-free yet still not real heap
    // pointers (e.g. a stray `0x20_0000_0203` left in a miscompiled object's
    // keys_array slot), which then fault inside `js_array_length`'s GC-header
    // read. Gate on the arena-bounds predicate (same one `js_object_create`
    // uses for prototype validation) so a garbage slot is treated as "no keys
    // array" instead of crashing the process. (#321: defends against the
    // Effect `makeGenericTag` mis-tagged-receiver corruption.)
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 || !is_valid_obj_ptr(keys as *const u8) {
        return;
    }
    // Check if key already exists. Past the sidecar threshold, probe the same
    // key→slot hash index the [[Set]] fast path maintains instead of scanning:
    // repeated `Object.defineProperty` on one object (Babel-style
    // `exports` re-export modules install hundreds of getters) made this
    // linear dup-check O(N²) total — the dominant cost of pi's module init
    // under perry (a single @babel/types re-export module took ~292ms vs
    // node's 3ms). A sidecar miss is authoritative here exactly as it is for
    // the [[Set]] append path ("the sidecar would have found it if it
    // existed"), and the append below records the new key via
    // `keys_index_insert` so the index stays fresh across the loop.
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count >= super::super::KEYS_INDEX_THRESHOLD as usize {
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key).byte_len as usize;
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let key_hash = super::super::key_bytes_hash(name_ptr, name_len);
        if super::super::keys_index_lookup(obj, keys, name_bytes, key_hash).is_some() {
            return; // already present
        }
    } else {
        let (slots, slot_len) = super::super::keys_array_dense_slots(keys);
        for i in 0..key_count.min(slot_len) {
            let stored = JSValue::from_bits((*slots.add(i)).to_bits());
            // #1781: SSO-aware match — pre-fix an existing inline-SSO key
            // wasn't seen here, so `Object.defineProperty(obj, "id", ...)`
            // on an object that already had `id` as an SSO key
            // double-inserted instead of overwriting.
            if crate::string::js_string_key_matches(stored, key) {
                return; // already present
            }
        }
    }
    // #10287: `Object.defineProperty` adding a NEW key performs the same
    // STRUCTURAL append as an ordinary `[[Set]]`, so it can reuse the learned
    // shape transition instead of minting a private keys array per receiver.
    // Without this every zod schema instance — whose constructor defines
    // `_zod` before installing ~60 methods by assignment — starts a private
    // lineage on its first define and can never again share a shape, a
    // transition edge or a keys array with its siblings.
    //
    // The edge is purely structural. Descriptor semantics are published
    // separately by the caller's `set_property_attrs`, whose keyed semantic
    // transition gives every receiver performing the same install the same
    // successor shape.
    let transition_eligible = define_append_transition_eligible(obj, keys);
    let mut interned_handle = None;
    let mut prev_shape_id = 0u32;
    if transition_eligible {
        let interned = intern_define_key(&scope, key);
        refresh_define_property_roots!();
        if interned.is_some() {
            // Interning can collect, so the keys edge and the receiver's stamp
            // are re-read here rather than reused from above.
            if crate::object::object_keys_array(obj) == keys {
                prev_shape_id = super::super::shapes::object_shape_stamp(obj);
                interned_handle = interned;
            }
        }
    }
    if let (Some(handle), true) = (interned_handle.as_ref(), prev_shape_id != 0) {
        let probe = handle.with_const_ptr::<crate::StringHeader, _>(|interned| {
            super::super::transition_cache_lookup(prev_shape_id, interned)
        });
        if let Some((next_keys, slot_idx, target_shape_id)) = probe {
            let live = crate::object::object_live_slot_count(obj);
            let alloc_limit = std::cmp::max(live, crate::object::INLINE_SLOT_FLOOR as u32);
            // An overflow target stays on the private path below for a
            // KEYS-ONLY install (an accessor claiming its slot): it writes no
            // value, so the overflow entry such an edge implies would never be
            // created. A data-descriptor claim does write it, so the objection
            // does not apply and refusing it forked every receiver (#10868).
            if next_keys != 0
                && (writes_value || slot_idx < alloc_limit)
                && cached_target_fits(target_shape_id, alloc_limit)
            {
                if !super::super::shapes::install_cached_object_shape_transition(
                    obj,
                    prev_shape_id,
                    target_shape_id,
                    next_keys as *mut ArrayHeader,
                ) {
                    set_object_keys_array(obj, next_keys as *mut ArrayHeader);
                }
                // Only an INLINE slot advances the live bound. An
                // overflow-located adoption must not, or the descriptor would
                // claim payload slots past the allocation and the collector
                // would trace them (`cached_target_fits`' hazard, from the
                // other side).
                if slot_idx >= live && slot_idx < alloc_limit {
                    set_object_live_slot_count(obj, slot_idx + 1);
                }
                return;
            }
        }
    }

    // #10868 step 2.5 stage 1b: the canonical successor for this ordered list
    // plus `key`. The clone-before-append this replaces existed because the
    // array might be shared; every keys array is shared now, so the question
    // is not asked — and the clone it used to answer with was a fresh address
    // per receiver, which is a fresh LAYOUT per receiver. This site is the
    // census's `object_ops/keys_array.rs:318`, 5,334 `key_count` mints, and
    // it is the mechanism named in the #10287 comment above: a receiver that
    // defines one key can never again share a keys array with its siblings.
    let canonical_parent = crate::object::canonical_keys::canonicalize(keys, key_count as u32);
    refresh_define_property_roots!();
    let new_keys = crate::object::canonical_keys::extend_key(canonical_parent, key).as_ptr();
    refresh_define_property_roots!();
    set_object_keys_array(obj, new_keys);
    // Keep the sidecar fresh (mirrors the [[Set]] append path): the entry is
    // keyed by the OBJECT address and length-stamped, so this contiguous
    // insert lets the next probe answer without a rebuild. No-op below the
    // index threshold or when no entry exists yet.
    {
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_hash = super::super::key_bytes_hash(name_ptr, (*key).byte_len as usize);
        super::super::keys_index_insert(
            crate::object::object_keys_array(obj),
            key_count as u32 + 1,
            key_hash,
            key_count as u32,
        );
    }
    // `field_count` is the inline/overflow boundary consulted by the read path
    // (`js_object_get_field`: index < field_count ⇒ read inline slot, else the
    // overflow map). It must never exceed the object's physically-allocated
    // inline capacity, which is `max(field_count, 8)` (see `js_object_alloc`).
    // Only bump it when this key genuinely lands in an in-bounds inline slot.
    //
    // A keys-only entry — a built-in accessor like `Map.prototype.size`, or a
    // key whose data spilled to the overflow map — must NOT push field_count
    // past the inline region. Doing so reclassifies already-overflowed (or
    // out-of-bounds) slots as inline, so later reads dereference past the
    // allocation into adjacent-heap garbage. That is what made
    // `Map.prototype.set` / `.values` read back as raw non-pointer values and
    // crash the reflective `.call` dispatch (#4099): installing the `size`
    // getter here bumped field_count from 8 (the proto's physical capacity) to
    // 11, exposing the overflowed `values` slot and corrupting the boundary.
    let new_index = key_count as u32;
    // #8113: one bound probe, reused.
    let live_slots = crate::object::object_live_slot_count(obj);
    let inline_capacity = std::cmp::max(live_slots, crate::object::INLINE_SLOT_FLOOR as u32);
    if new_index < inline_capacity && new_index >= live_slots {
        set_object_live_slot_count(obj, new_index + 1);
    }
    // #10287: teach the edge this append just built, so the NEXT receiver with
    // the same predecessor shape takes the branch above instead of cloning a
    // private keys array of its own. Inline targets only, for the reason given
    // there. `transition_cache_insert` stamps `GC_FLAG_SHAPE_SHARED` on the
    // published array, so any later growth on either receiver clones first.
    #[cfg(feature = "shape-mint-diag")]
    crate::object::shape_mint_census::note_define_outcome(if interned_handle.is_none() {
        "no publish: no interned handle"
    } else if prev_shape_id == 0 {
        "no publish: prev_shape_id == 0"
    } else if new_index >= inline_capacity {
        "no publish: new_index >= inline_capacity"
    } else {
        "publish: reached the edge site"
    });
    if let (Some(handle), true) = (interned_handle.as_ref(), prev_shape_id != 0) {
        if writes_value || new_index < inline_capacity {
            let target_shape_id = super::super::shapes::object_shape_stamp(obj);
            let published_keys = crate::object::object_keys_array(obj);
            #[cfg(feature = "shape-mint-diag")]
            crate::object::shape_mint_census::note_define_outcome(if target_shape_id == 0 {
                "no publish: target_shape_id == 0"
            } else if target_shape_id == prev_shape_id {
                "no publish: target == prev"
            } else if published_keys.is_null() {
                "no publish: published keys null"
            } else {
                "PUBLISHED an edge"
            });
            if target_shape_id != 0 && target_shape_id != prev_shape_id && !published_keys.is_null()
            {
                handle.with_const_ptr::<crate::StringHeader, _>(|interned_key| {
                    super::super::transition_cache_insert(
                        std::ptr::null(),
                        prev_shape_id,
                        interned_key,
                        published_keys as usize,
                        new_index,
                        target_shape_id,
                    )
                });
            }
        }
    }
}

/// Does the cached target's live inline-slot bound fit THIS receiver?
///
/// A transition edge is keyed by the predecessor SHAPE, and two receivers can
/// share a shape (notably the keyless birth shape) while holding different
/// physical inline capacities — the allocator sizes them from their birth
/// field count. Adopting a target whose bound exceeds this receiver's
/// allocation would publish payload slots past the end of the object: the
/// collector traces that range, so the next collection reads (and rewrites)
/// memory the object does not own. Keep such a receiver on the private append.
unsafe fn cached_target_fits(target_shape_id: u32, alloc_limit: u32) -> bool {
    super::super::shapes::shape_descriptor_by_id(target_shape_id)
        .is_some_and(|target| target.live_inline_slot_count <= alloc_limit)
}

/// Intern `key` for a transition-cache probe, rooted in the caller's scope.
/// Interning can collect, so the caller refreshes its own roots afterwards.
unsafe fn intern_define_key<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    key: *const crate::StringHeader,
) -> Option<crate::gc::RuntimeHandle<'scope>> {
    let key_gc = (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let interned = if (*key_gc).gc_flags & crate::gc::GC_FLAG_INTERNED != 0 {
        key
    } else {
        let hash = super::super::key_content_hash(key);
        crate::string::js_string_intern(key, hash)
    };
    (!interned.is_null()).then(|| scope.root_string_ptr(interned))
}

/// Is `obj` an ordinary class-less receiver whose `defineProperty` key append
/// is exactly the append the `[[Set]]` transition lattice already models
/// (#10287)? Conservative: anything with its own layout rules — a class
/// instance or class object, a native-module receiver, a reserved-slot floor,
/// `Object.prototype`, a URL, a typed array, an exotic expando host, a
/// tombstoned or non-ordinary shape, or a keys edge that does not match the
/// receiver's published descriptor — keeps the private append.
unsafe fn define_append_transition_eligible(
    obj: *mut ObjectHeader,
    keys: *const ArrayHeader,
) -> bool {
    let Some(gc) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        #[cfg(feature = "shape-mint-diag")]
        crate::object::shape_mint_census::note_define_outcome("ineligible: no gc header");
        return false;
    };
    const BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
        | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES;
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc._reserved & BLOCKING != 0
        || !crate::object::object_is_regular(obj)
    {
        #[cfg(feature = "shape-mint-diag")]
        crate::object::shape_mint_census::note_define_outcome(
            if gc.obj_type != crate::gc::GC_TYPE_OBJECT {
                "ineligible: not GC_TYPE_OBJECT"
            } else if gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
                "ineligible: forwarded"
            } else if gc._reserved & BLOCKING != 0 {
                "ineligible: frozen/sealed/noextend/taproto/tombstones"
            } else {
                "ineligible: not object_is_regular"
            },
        );
        return false;
    }
    let class_id = (*obj).class_id;
    // A class INSTANCE is eligible — the `[[Set]]` tail already learns and
    // replays transition edges for class receivers. A class OBJECT is not: its
    // writes must reach the `mirror_class_object_static_write` completions.
    if class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || crate::object::class_registry::is_class_object_ptr(obj.cast())
        || crate::object::reserved_slot_floor_for_class_id(class_id) != 0
        || crate::array::object_prototype_addr_matches(obj as usize)
        || crate::url::is_url_object_shape(obj)
        || crate::typedarray::lookup_typed_array_kind(obj as usize).is_some()
    {
        #[cfg(feature = "shape-mint-diag")]
        crate::object::shape_mint_census::note_define_outcome(
            "ineligible: class object / native / reserved / proto / url / TA",
        );
        return false;
    }
    let value = crate::value::js_nanbox_pointer(obj as i64);
    if crate::object::exotic_expando::exotic_expando_kind_of_value(value).is_some() {
        #[cfg(feature = "shape-mint-diag")]
        crate::object::shape_mint_census::note_define_outcome("ineligible: exotic expando");
        return false;
    }
    if crate::object::prototype_chain::object_has_prototype_divergence(obj as usize) {
        #[cfg(feature = "shape-mint-diag")]
        crate::object::shape_mint_census::note_define_outcome("ineligible: prototype divergence");
        return false;
    }
    let verdict = match super::super::shapes::object_shape_descriptor(obj) {
        // A keyless receiver has no descriptor yet on some paths; the tail
        // learns its keyless→one-key edge from the same stamp.
        None => keys.is_null(),
        Some(shape) => shape.hole_count == 0 && shape.keys == keys as u64,
    };
    #[cfg(feature = "shape-mint-diag")]
    crate::object::shape_mint_census::note_define_outcome(if verdict {
        "eligible"
    } else {
        match super::super::shapes::object_shape_descriptor(obj) {
            None => "ineligible: no descriptor and keys non-null",
            Some(shape) if shape.hole_count != 0 => "ineligible: shape has holes",
            _ => "ineligible: shape.keys != receiver keys",
        }
    });
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_property_key_growth_does_not_mutate_a_shared_shape_sibling() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"";
            let first =
                crate::object::js_object_alloc_with_shape(0x6B45_5901, 0, packed.as_ptr(), 0);
            let sibling =
                crate::object::js_object_alloc_with_shape(0x6B45_5901, 0, packed.as_ptr(), 0);
            assert_eq!(
                crate::object::object_keys_array(first),
                crate::object::object_keys_array(sibling)
            );

            // A logical-field/key-count mismatch is not evidence that the
            // keys array is privately owned. This was the false assumption in
            // the old clone condition.
            set_object_live_slot_count(first, 1);
            let sibling_shape = (*sibling).parent_class_id;
            let key = crate::string::js_string_from_bytes(b"ALIAS_KEYS".as_ptr(), 10);
            ensure_key_in_keys_array(first, key);

            assert!(own_key_present(first, key));
            assert!(!own_key_present(sibling, key));
            assert_ne!(
                crate::object::object_keys_array(first),
                crate::object::object_keys_array(sibling)
            );
            assert_ne!((*first).parent_class_id, sibling_shape);
            let sibling_descriptor = crate::object::shapes::shape_descriptor_by_id(sibling_shape)
                .expect("sibling descriptor must remain installed");
            assert_eq!(
                sibling_descriptor.keys,
                crate::object::object_keys_array(sibling) as u64
            );
            assert_eq!(sibling_descriptor.logical_key_count, 0);
            let first_descriptor =
                crate::object::shapes::shape_descriptor_by_id((*first).parent_class_id)
                    .expect("defineProperty growth must install an exact descriptor");
            assert_eq!(
                first_descriptor.keys,
                crate::object::object_keys_array(first) as u64
            );
            assert_eq!(first_descriptor.logical_key_count, 1);
            assert_eq!(first_descriptor.live_inline_slot_count, 1);
        }
    }

    #[test]
    fn shared_shape_key_clone_stays_all_pointer_without_a_side_mask() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            const KEY_COUNT: usize = 32;
            let mut packed = Vec::new();
            for i in 0..KEY_COUNT {
                packed.extend_from_slice(format!("field_{i:02}").as_bytes());
                packed.push(0);
            }
            let first = crate::object::js_object_alloc_with_shape(
                0x6B45_5902,
                KEY_COUNT as u32,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let sibling = crate::object::js_object_alloc_with_shape(
                0x6B45_5902,
                KEY_COUNT as u32,
                packed.as_ptr(),
                packed.len() as u32,
            );
            assert_eq!(
                crate::object::object_keys_array(first),
                crate::object::object_keys_array(sibling)
            );

            // The canonical shape array may already own a permanent mask;
            // cloning it must not add another per-object layout record.
            let tables_before = crate::gc::per_object_layout_table_sizes();
            let extra = crate::string::js_string_from_bytes(b"extra".as_ptr(), 5);
            ensure_key_in_keys_array(first, extra);

            let cloned = crate::object::object_keys_array(first);
            assert_ne!(cloned, crate::object::object_keys_array(sibling));
            assert_eq!((*cloned).length, KEY_COUNT as u32 + 1);
            assert!(own_key_present(first, extra));
            assert!(!own_key_present(sibling, extra));
            assert_eq!(crate::gc::per_object_layout_table_sizes(), tables_before);

            let header =
                (cloned as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            assert_ne!(
                (*header)._reserved & crate::gc::GC_LAYOUT_ALL_POINTERS,
                0,
                "the cloned keys array must retain its compact all-pointer layout"
            );
        }
    }
}

/// Install a built-in *getter-only* accessor on a prototype object so that
/// `Object.getOwnPropertyDescriptor(proto, key)` reflects it as a real
/// accessor descriptor `{ get, set: undefined, enumerable, configurable }`.
///
/// `getter_bits` is the NaN-boxed `f64` bits of the getter closure (0 = none).
/// The descriptor is non-enumerable and configurable, matching the ECMA-262
/// shape for `%TypedArray%.prototype` accessors like `length` / `byteLength` /
/// `byteOffset` / `buffer`. Reflection-only: this does NOT flip the hot-path
/// descriptor gate (see `set_builtin_accessor_descriptor`). #2060.
pub(crate) unsafe fn install_builtin_getter(proto: *mut ObjectHeader, key: &str, getter_bits: u64) {
    if proto.is_null() || (proto as usize) < 0x10000 {
        return;
    }
    let key_str = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    if key_str.is_null() {
        return;
    }
    // Make the key discoverable by `own_key_present` / `getOwnPropertyNames`.
    ensure_key_in_keys_array(proto, key_str);
    // Spec: an accessor getter's `.name` is `"get " + key` (e.g.
    // `Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"byteLength").get.name
    // === "get byteLength"`). Register it against the getter closure's func_ptr;
    // without this the `.name` read returned `""`.
    let getter_ptr = (getter_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if getter_ptr >= 0x1000 && crate::closure::is_closure_ptr(getter_ptr) {
        let func_ptr = (*(getter_ptr as *const crate::closure::ClosureHeader)).func_ptr as usize;
        crate::builtins::register_function_name_if_absent(func_ptr, &format!("get {key}"));
    }
    set_builtin_accessor_descriptor(
        proto as usize,
        key.to_string(),
        AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        // writable is N/A for an accessor; enumerable=false, configurable=true.
        PropertyAttrs::new(true, false, true),
    );
}

/// O(1) own-key presence via the [[Set]]-path sidecar, for the
/// `Object.defineProperty` flow (#6743). Returns `Some(present)` when the
/// sidecar is applicable — a genuine wide object (keys past
/// `KEYS_INDEX_THRESHOLD`) with a valid heap string key — using the same
/// authoritative-miss trust model as the fast [[Set]] append. Returns `None`
/// when not applicable (caller falls back to the linear `own_key_present` /
/// `obj_value_has_own_key`). Native-module namespaces expose VIRTUAL keys
/// that never live in `keys_array`, so they always fall back.
pub(crate) unsafe fn own_key_present_via_index(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<bool> {
    if obj.is_null() || key.is_null() {
        return None;
    }
    // Centralized address classification (rejects handle bands / non-heap /
    // header-less slab addrs without dereferencing) + GC-type brand check —
    // both for the receiver and for whatever the keys_array slot holds (a
    // corrupted slot must classify as "no keys array", not fault; #321/#3527).
    match crate::value::addr_class::try_read_gc_header(obj as usize) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {}
        _ => return None,
    }
    if (*obj).class_id == super::super::native_module::NATIVE_MODULE_CLASS_ID {
        return None;
    }
    if super::super::string_wrapper::has_index_key(obj as usize, key) {
        return Some(true);
    }
    let keys = crate::object::object_keys_array(obj);
    match crate::value::addr_class::try_read_gc_header(keys as usize) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => {}
        _ => return None,
    }
    let key_count = crate::array::js_array_length(keys);
    if key_count < super::super::KEYS_INDEX_THRESHOLD {
        return None;
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
    let key_hash = super::super::key_bytes_hash(name_ptr, name_len);
    match super::super::shapes::shape_slot_lookup_verdict(
        keys, name_bytes, key_hash, key_count, true,
    ) {
        super::super::shapes::KeysIndexVerdict::Found(_) => Some(true),
        super::super::shapes::KeysIndexVerdict::Absent => Some(false),
        // A shortened or otherwise incomplete index cannot prove absence.
        // Preserve the caller's exact fallback instead of turning a stale miss
        // into a false negative.
        super::super::shapes::KeysIndexVerdict::Unindexed => None,
    }
}

/// Helper: does `key` appear in `obj.keys_array`?
pub(crate) unsafe fn own_key_present(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> bool {
    // Every GC allocation is `align.max(8)`-aligned, so a real object pointer
    // has its low 3 bits clear. Rejecting misaligned `obj` keeps a non-object
    // value (e.g. a native-module namespace sentinel reaching `hasOwnProperty`
    // via a caller that didn't route through `extract_obj_ptr`) from being
    // dereferenced as an ObjectHeader. (#3527)
    if obj.is_null() || (obj as usize) < 0x10000 || (obj as usize) & 0x7 != 0 || key.is_null() {
        return false;
    }
    if let Some(present) = crate::process::process_env_has_field(obj, key) {
        return present;
    }
    // Only a genuine `GC_TYPE_OBJECT` carries a ShapeId that can derive a keys
    // array. A non-object receiver that still cleared the alignment/range
    // guard above — most importantly a real `Map`/`Set`, whose 16-byte header
    // is only `size`/`capacity`/`entries` — has no such field, so reading
    // `crate::object::object_keys_array(obj)` loads 8 bytes past the header into the adjacent
    // allocation. A `Map` reaching `js_object_get_field_by_name`'s `.size`
    // fast path (an `any`-typed `map.size` dispatched by name) did exactly
    // that: the out-of-bounds word was a live neighbour's GC-header value,
    // which cleared the keys-pointer guard below and then SIGBUS'd on the
    // `[keys-8]` type-tag read. `try_read_gc_header` rejects header-less slab
    // allocations and non-heap addresses without dereferencing them. A
    // non-object has no own string keys, so answer false and let the caller
    // fall through to the type-specific tail (which serves e.g. `Map.size`).
    match crate::value::addr_class::try_read_gc_header(obj as usize) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {}
        _ => return false,
    }
    if super::super::string_wrapper::has_index_key(obj as usize, key) {
        return true;
    }
    let keys = crate::object::object_keys_array(obj);
    if keys.is_null() {
        return false;
    }
    let keys_ptr = keys as usize;
    // Same alignment invariant for the derived keys-array pointer: when `obj` is not a
    // genuine object its would-be shape token is garbage that may land in the
    // address range yet be misaligned. Without this guard the `[keys-8]`
    // GcHeader read below SIGBUSes on that garbage. (#3527)
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 || keys_ptr & 0x7 != 0 {
        return false;
    }
    // Validate keys_array GC header
    let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
        return false;
    }
    let key_count = crate::array::js_array_length(keys);
    // The shared helper distinguishes a complete index miss from an index that
    // cannot answer. Complete misses are authoritative, so Object.assign's
    // growing destination does not scan every preceding key before appending;
    // stale/incomplete indexes retain the dense-slot correctness fallback.
    // Slots and counts are u32 throughout, so there is no 65,536-key ceiling.
    super::super::keys_find_slot_by_key_ptr(keys, key_count, key).is_some()
}
