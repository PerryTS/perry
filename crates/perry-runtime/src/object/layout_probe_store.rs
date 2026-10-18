use super::*;

/// The probe's presence mode (`in`, `Object.hasOwn`,
/// `hasOwnProperty`): `1` when `key` is present on the object at `obj_bits`
/// (own, or along its prototype chain unless `own_only`), `0` when the
/// layouts prove it absent, `-1` when the probe cannot answer (an exotic
/// receiver, an index-like or uninterned key, a builtin prototype's unlisted
/// names, an unknown prototype). A hole slot is an absent key (the key view);
/// an accessor is present. GC leaf.
///
/// # Safety
/// `obj_bits` and `key_bits` any NaN-boxed values.
pub(crate) unsafe fn probe_has(obj_bits: u64, key_bits: u64, own_only: bool) -> i32 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return -1;
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return -1;
    }
    // A key with no property atom (its text was never interned as one) can
    // still match a list word written as that very string: presence only,
    // since a miss cannot prove absence for an uninterned key.
    let (key, hash, exact) = match computed_key(key_bits) {
        Some((k, h)) => (k, h, true),
        None if key_bits & TAG_MASK == STRING_TAG => {
            let s = (key_bits & POINTER_MASK) as *const crate::StringHeader;
            let text =
                std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
            if index_like(text) {
                return -1;
            }
            (
                key_bits,
                crate::object::key_bytes_hash(text.as_ptr(), text.len()),
                false,
            )
        }
        None => return -1,
    };
    let absent = if exact { 0 } else { -1 };
    let dir = super::super::ordinary_dir_addr();
    let mut cur = payload as *const ObjectHeader;
    for _ in 0..MAX_HOPS {
        let w = (*cur).parent_class_id;
        let r = record_of(dir, w);
        if r.is_null() {
            return -1;
        }
        match own_position(r, key, hash) {
            Ok(pos) => {
                let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
                let entry = crate::object::key_attrs::keys_entry(keys, pos);
                // A private name shares its spelling with a public key; the
                // complete operation tells them apart.
                if entry & crate::object::key_attrs::ENTRY_PRIVATE != 0 {
                    return -1;
                }
                if entry & crate::object::key_attrs::ENTRY_ACCESSOR_MASK != 0 {
                    return 1;
                }
                let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
                let v = if slot < (*r).live_inline_slot_count {
                    *((cur as *const u8)
                        .add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
                        as *const u64)
                } else {
                    crate::object::spill::spill_get_present(cur as usize, slot as usize)
                        .unwrap_or(TAG_HOLE)
                };
                if v != TAG_HOLE {
                    return 1;
                }
                // A hole slot: the key is absent here; a re-add lives in the
                // overflow.
            }
            Err(false) => return -1,
            // A class declaration's prototype holds its symbol-named
            // declared methods as virtual own keys its list does not name:
            // its list proves no symbol absent.
            Err(true)
                if key & TAG_MASK != STRING_TAG
                    && (*cur).class_id != 0
                    && class_prototype_object(cur) =>
            {
                return -1
            }
            Err(true) => {}
        }
        if w & crate::object::overflow::SHADOW_BIT != 0 {
            let meta = (*cur).meta;
            if !meta.is_null() && (*meta).overflow != 0 {
                let t = (*meta).overflow as usize as *const crate::object::overflow::Table;
                if let Some(i) = crate::object::overflow::find_hashed(t, key, hash) {
                    return if *crate::object::overflow::attrs_at(t, i) as u8
                        & crate::object::key_attrs::ENTRY_PRIVATE
                        != 0
                    {
                        -1
                    } else {
                        1
                    };
                }
            }
        }
        // Absence on `cur` is proven only by a layout that lists every own
        // key (the builtin prototypes are static layouts).
        if !walks_from(cur, r) {
            return -1;
        }
        if own_only {
            // An Array subclass's elements store holds its own index keys and
            // `length`, which no key list names.
            let meta = (*cur).meta;
            if !meta.is_null() && (*meta).elements != 0 {
                return -1;
            }
            return absent;
        }
        cur = match prototype_of(dir, cur, r) {
            Next::Object(p) => p,
            Next::End => return absent,
            Next::Unknown => return -1,
        };
    }
    -1
}

/// Is `obj` (class id nonzero) a class declaration's or a class
/// evaluation's prototype object?
#[cold]
#[inline(never)]
pub(super) unsafe fn class_prototype_object(obj: *const ObjectHeader) -> bool {
    crate::object::class_value::class_decl_prototype_link((*obj).class_id) as usize == obj as usize
        || crate::object::field_get_set::class_evaluation_prototype_class_id(obj as usize).is_some()
}

/// The store twin of [`perry_probe_get`] (§3.7, the stores' probe path):
/// `obj.key = value` when the receiver's own layout holds `key` as a plain
/// writable data property in an inline slot, or its overflow holds it as a
/// plain writable entry of its own: one barriered store. Returns 1 when it
/// stored. Returns 0 for everything the site's complete `[[Set]]` must do:
/// a key add (the canonical tree or the overflow), a setter or a
/// non-writable property along the chain, a hole slot (an absent key, per
/// the key view), a frozen, exotic or per-object receiver, a slot whose
/// representation the value would widen.
///
/// GC leaf: no allocation on the heap, no collection, no user code.
///
/// # Safety
/// `dir` as for [`perry_probe_get`]; `obj_bits` any NaN-boxed value;
/// `key_bits` a property atom or a symbol; `hash` its hash.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_set(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    hash: u64,
    value: f64,
) -> i32 {
    probe_set::<true>(dir, obj_bits, key_bits, hash, value)
}

/// Computed stores may overwrite the immutable layout or overflow, but keys
/// outside that layout keep their ordinary computed-key overflow admission.
/// The named store retains its existing canonical key-add operation.
#[inline(always)]
pub(super) unsafe fn probe_set<const ALLOW_ADDS: bool>(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    hash: u64,
    value: f64,
) -> i32 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return 0;
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return 0;
    }
    let obj = payload as *mut ObjectHeader;
    let r = record_of(dir, (*obj).parent_class_id);
    if r.is_null() || (*r).object_kind() != super::super::ShapeObjectKind::Ordinary {
        return 0;
    }
    let gc = (payload - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return 0;
    }
    let bits = if value.to_bits() == POINTER_TAG {
        crate::value::TAG_UNDEFINED
    } else {
        value.to_bits()
    };
    match own_position(r, key_bits, hash) {
        Ok(pos) => {
            let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
            if (*r).summary()
                & (crate::object::key_attrs::SUMMARY_BLOCKS_STORE
                    | crate::object::key_attrs::SUMMARY_PRIVATE)
                != 0
                && !crate::object::key_attrs::entry_is_plain_writable_data(
                    crate::object::key_attrs::keys_entry(keys, pos),
                )
            {
                return 0;
            }
            let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
            if slot >= (*r).live_inline_slot_count {
                // A spilled key: an overwrite of its present value in the
                // spill storage the object already has (a hole is an absent
                // key, which `[[Set]]` adds).
                return i32::from(crate::object::spill::spill_overwrite_present(
                    obj as usize,
                    slot as usize,
                    bits,
                ));
            }
            let address = (obj as *mut u8)
                .add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
                .cast::<u64>();
            // A hole slot: the key is absent (the key view). [[Set]] adds it
            // in place, in the layout's key order, when nothing on the chain
            // intercepts it and the object takes new keys without an order
            // authority of its own (an overflow, a stable-tombstone record).
            if *address == TAG_HOLE && !hole_fill_admitted(dir, obj, r, keys, pos, key_bits, hash) {
                return 0;
            }
            let rep = crate::object::field_rep::slot_rep((*r).rep, slot);
            if bits & 0x7ff0_0000_0000_0000 != 0x7ff0_0000_0000_0000
                && rep != crate::object::field_rep::REP_SPECIAL
            {
                crate::object::builtin_proto::note_value_store(
                    obj,
                    slot as usize,
                    false,
                    "probe store",
                );
                // GC_STORE_AUDIT(POINTER_FREE): a canonical finite Number.
                address.write(bits);
                return 1;
            }
            if rep == crate::object::field_rep::REP_ANY {
                crate::object::builtin_proto::note_value_store(
                    obj,
                    slot as usize,
                    false,
                    "probe store",
                );
                crate::gc::runtime_store_jsvalue_slot_layout_deferred(
                    obj as usize,
                    address as usize,
                    slot as usize,
                    bits,
                );
                return 1;
            }
            0
        }
        Err(true) => {
            let meta = (*obj).meta;
            if meta.is_null() || (*meta).overflow == 0 {
                // The key is absent from a layout that lists every own key:
                // a key add, when the chain admits it (§3.7).
                return i32::from(ALLOW_ADDS && probe_add(dir, obj, r, key_bits, hash, bits));
            }
            let t = (*meta).overflow as usize as *mut crate::object::overflow::Table;
            let Some(i) = crate::object::overflow::find_hashed(t, key_bits, hash) else {
                return 0;
            };
            if !crate::object::overflow::plain_writable_value(t, i) {
                return 0;
            }
            let slot = crate::object::overflow::value_at(t, i);
            // GC_STORE_AUDIT(BARRIERED): the table's own value word.
            *slot = bits;
            crate::gc::runtime_write_barrier_slot(t as usize, slot as usize, bits);
            1
        }
        Err(false) => 0,
    }
}

/// May the store probe fill hole slot `pos` (an absent key, the key view) of
/// `obj` (record `r`, key list `keys`) in place? That fill is the key's add
/// (`layout_desc::family_add`'s in-order case): the object takes new keys
/// (extensible, no overflow or stable-tombstone record ordering it, no
/// object inherits from it), no key after `pos` in the list is present (the
/// key position is then the insertion order), and nothing on the prototype
/// chain intercepts the add ([`chain_admits_add`]). Any other fill is
/// `[[Set]]`'s (`layout_desc::family_store`). GC leaf.
#[inline(never)]
pub(super) unsafe fn hole_fill_admitted(
    dir: *const u8,
    obj: *mut ObjectHeader,
    r: *const ShapeRecord,
    keys: *const crate::array::ArrayHeader,
    pos: u32,
    key_bits: u64,
    hash: u64,
) -> bool {
    let gc = crate::gc::header_from_trusted_user_ptr(obj as *const u8);
    if (*gc)._reserved
        & (crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES)
        != 0
        || (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT != 0
    {
        return false;
    }
    let meta = (*obj).meta;
    if !meta.is_null() && (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE != 0 {
        return false;
    }
    !crate::object::layout_desc::later_late_key_present(obj, keys, (*r).logical_key_count, pos)
        && chain_admits_add(dir, obj, r, key_bits, hash)
}

/// The store probe's key add: `obj` (record `r`, an ordinary layout that
/// proves `key_bits` absent, no overflow) takes `key` as a new own data
/// property when its flags let it grow, nothing on its prototype chain
/// intercepts the add ([`chain_admits_add`], from the layout words alone),
/// and the canonical key-add edge from its layout is published with an
/// inline slot for the key (`field_set_by_name::leaf_key_add`). `false`,
/// having changed nothing, for everything else: `[[Set]]` adds it.
///
/// GC leaf, like the probe that calls it.
#[inline(never)]
pub(super) unsafe fn probe_add(
    dir: *const u8,
    obj: *mut ObjectHeader,
    r: *const ShapeRecord,
    key_bits: u64,
    hash: u64,
    bits: u64,
) -> bool {
    if key_bits & TAG_MASK != STRING_TAG {
        return false;
    }
    let gc = crate::gc::header_from_trusted_user_ptr(obj as *const u8);
    if (*gc)._reserved
        & (crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
            | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
        != 0
        || (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT != 0
        || (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
        // A family object keeps its layout word for life: a key its layout
        // does not name joins its overflow (`layout_desc::family_store`).
        || crate::object::layout_desc::is_family_layout_word((*obj).parent_class_id)
    {
        return false;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags
                & (crate::object::OBJECT_META_FLAG_IS_PROTOTYPE
                    | crate::object::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC
                    | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
                != 0)
    {
        return false;
    }
    if object_prototype(dir) == Some(obj as *const ObjectHeader)
        || !chain_admits_add(dir, obj, r, key_bits, hash)
    {
        return false;
    }
    crate::object::leaf_key_add(
        obj,
        (key_bits & POINTER_MASK) as *const crate::StringHeader,
        bits,
    )
}

/// [`probe_add`]'s question for the generic store's own key-add lane
/// (`shape_store::try_warm_transition`): may `obj` take `key` (a heap string)
/// as a new own data property, by its layout and the layouts of its
/// prototype chain? `Some(true)`: proven (the key is absent from a complete
/// own list, no overflow, nothing on the chain intercepts); `Some(false)`:
/// proven intercepted (an own or inherited setter or non-writable); `None`:
/// the layouts cannot say.
///
/// # Safety
/// `obj` a live ordinary `ObjectHeader`; `key` a live heap string.
pub(crate) unsafe fn layouts_admit_add(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<bool> {
    let (key_bits, hash) = computed_key(STRING_TAG | key as u64)?;
    let dir = super::super::ordinary_dir_addr();
    let r = record_of(dir, (*obj).parent_class_id);
    if r.is_null() || !(*r).object_kind().is_ordinary_layout() {
        return None;
    }
    let meta = (*obj).meta;
    if !meta.is_null() && (*meta).overflow != 0 {
        return None;
    }
    if own_position(r, key_bits, hash) != Err(true) {
        return None;
    }
    chain_admits_add(dir, obj, r, key_bits, hash).then_some(true)
}

/// The property-atom hash of `__proto__` (FNV-1a, `key_bytes_hash`).
const PROTO_KEY_HASH: u64 = {
    let bytes = b"__proto__";
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
};

/// Does `obj`'s prototype chain let `[[Set]]` add `key` as an own data
/// property of `obj` (record `r`)? No accessor and no non-writable property
/// of that name on any prototype before one that holds it as plain writable
/// data (which the add shadows), proved from the layouts. `false` whenever
/// the layouts cannot prove it. GC leaf.
pub(super) unsafe fn chain_admits_add(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *const ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> bool {
    let (mut cur, mut rc) = (obj, r);
    for _ in 0..MAX_HOPS {
        if !walks_from(cur, rc) {
            return false;
        }
        let next = match prototype_of(dir, cur, rc) {
            Next::Object(p) => p,
            Next::End => return true,
            Next::Unknown => return false,
        };
        let w = (*next).parent_class_id;
        // An unwritten `%Object.prototype%` ends the chain, and its one
        // accessor is `__proto__` (a key of that hash takes the walk).
        if hash != PROTO_KEY_HASH
            && w < crate::object::layout_desc::UNSHADOWED_WORD_END
            && object_prototype(dir) == Some(next)
        {
            return true;
        }
        let rn = record_of(dir, w);
        if rn.is_null() {
            return false;
        }
        // A layout that holds no accessor, no non-writable and no private
        // key cannot intercept the add whatever it holds (a plain writable
        // data property of the name is shadowed by it): its key list is not
        // asked. Its overflow, below, still is.
        let blocks = (*rn).summary()
            & (crate::object::key_attrs::SUMMARY_BLOCKS_STORE
                | crate::object::key_attrs::SUMMARY_PRIVATE)
            != 0;
        match if blocks {
            own_position(rn, key_bits, hash)
        } else {
            Err(true)
        } {
            Ok(pos) => {
                let keys = (*rn).keys as usize as *const crate::array::ArrayHeader;
                let entry = crate::object::key_attrs::keys_entry(keys, pos);
                if !crate::object::key_attrs::entry_is_plain_writable_data(entry) {
                    return false;
                }
                if value_at(next, rn, pos).to_bits() != TAG_HOLE {
                    return true;
                }
            }
            Err(false) => return false,
            Err(true) => {}
        }
        if w & crate::object::overflow::SHADOW_BIT != 0 {
            let meta = (*next).meta;
            if !meta.is_null() && (*meta).overflow != 0 {
                let t = (*meta).overflow as usize as *const crate::object::overflow::Table;
                if let Some(i) = crate::object::overflow::find_hashed(t, key_bits, hash) {
                    return crate::object::key_attrs::entry_is_plain_writable_data(
                        *crate::object::overflow::attrs_at(t, i) as u8,
                    );
                }
            }
        }
        cur = next;
        rc = rn;
    }
    false
}

/// [`perry_probe_get`] for a site that has not tested its receiver (the
/// outlined read, `codegen::full_outline_ic_enabled`): any value bits;
/// `TAG_HOLE` for everything but an object pointer above the handle band.
///
/// # Safety
/// As [`perry_probe_get`], with `obj_bits` any NaN-boxed value.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_value(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        /// Object model step 2c: may a family constructor's run of stores, from the
        /// store of key position `pos` (atom hash `hash`) on, fill `obj`'s hole slots
        /// in place, as plain adds in key order? Codegen asks once per run: between
        /// two points where user code can run, nothing can change the answer, so the
        /// run's stores (its keys at increasing positions of the family, `expr/
        /// layout_read.rs`) are plain slot stores while it holds. It holds when
        ///
        /// * `obj` is a family object of class `class_id` (the constructor's) or of
        ///   a family descending from it, at its layout word without the shadow bit
        ///   (no overflow), extensible, not a prototype, and its key at `pos` is the
        ///   store's (codegen gives every such layout the run's keys at the same
        ///   positions and slots);
        /// * no key after `pos` is present (an add at `pos` is then in key order,
        ///   and so is every later add of the run); `order_known` (nonzero) when
        ///   codegen proves it: no code since the birth could have stored a key;
        /// * no prototype of `obj` intercepts an add of any of the family's keys
        ///   ([`chain_admits_family`]): one record read per hop.
        ///
        /// Its keys then also read as plain data: no accessor on the chain names
        /// one, which is what lets the run read `this.k` without ending it.
        ///
        /// GC leaf: no allocation, no collection, no user code, no throw.
        ///
        /// # Safety
        /// `dir` as for [`perry_probe_get`]; `obj_bits` any NaN-boxed value.
        #[no_mangle]
        pub unsafe extern "C" fn perry_family_run_admits(
            dir: *const u8,
            obj_bits: u64,
            hash: u64,
            pos: u32,
            order_known: u32,
            class_id: u32,
        ) -> i32 {
            if obj_bits & TAG_MASK != POINTER_TAG {
                return 0;
            }
            let payload = (obj_bits & POINTER_MASK) as usize;
            if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
                return 0;
            }
            let obj = payload as *mut ObjectHeader;
            let word = (*obj).parent_class_id;
            if word & crate::object::overflow::SHADOW_BIT != 0 {
                note_run(1);
                return 0;
            }
            let Some(d) = crate::object::layout_desc::family_desc_of(obj) else {
                note_run(1);
                return 0;
            };
            if !crate::object::layout_desc::family_descends(d, class_id) {
                note_run(2);
                return 0;
            }
            if pos >= d.key_count
                || d.keys.is_null()
                || (*(*d.keys.add(pos as usize)).atom).hash != hash
                || d.slot_count > crate::object::object_inline_capacity(obj)
            {
                note_run(3);
                return 0;
            }
            const _: () = assert!(
                crate::codegen_abi::OBJ_FLAGS_REFUSE_FAMILY_RUN
                    == crate::gc::OBJ_FLAG_FROZEN
                        | crate::gc::OBJ_FLAG_SEALED
                        | crate::gc::OBJ_FLAG_NO_EXTEND
                        | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
            );
            let gc = crate::gc::header_from_trusted_user_ptr(obj as *const u8);
            if (*gc)._reserved & crate::codegen_abi::OBJ_FLAGS_REFUSE_FAMILY_RUN != 0 {
                note_run(4);
                return 0;
            }
            let meta = (*obj).meta;
            if !meta.is_null()
                && ((*meta).overflow != 0
                    || (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE != 0)
            {
                note_run(4);
                return 0;
            }
            // The order: every key after `pos` still holds its birth's hole. A run
            // that starts before any code since the birth could run (`order_known`,
            // the constructors' inert prologue) knows it.
            if order_known == 0 {
                let fields =
                    (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
                let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
                if keys[pos as usize + 1..]
                    .iter()
                    .any(|k| *fields.add(k.slot as usize) != TAG_HOLE)
                {
                    note_run(5);
                    return 0;
                }
            }
            let r = record_of(dir, word);
            if r.is_null() {
                note_run(7);
                return 0;
            }
            let admits = chain_admits_family(dir, obj, r, d);
            note_run(if admits { 0 } else { 6 });
            i32::from(admits)
        }

        /// Does any prototype of `obj` (record `r`, a family object of descriptor
        /// `d`) hold an accessor, a non-writable or a private property named like
        /// one of `d`'s keys, or is the chain one the records cannot answer? `true`
        /// when none does. Each hop is decided by its layout word: its record's
        /// attribute summary (a hop with no such entry, the common class prototype
        /// of methods, costs that one test), else the hashes of its blocking entries
        /// that its key index lists, each one probe of `d`'s hash index. A hop with
        /// an overflow (the shadow bit) answers from the overflow's entries. GC leaf.
        unsafe fn chain_admits_family(
            dir: *const u8,
            obj: *const ObjectHeader,
            r: *const ShapeRecord,
            d: &crate::object::layout_desc::PerryLayoutDesc,
        ) -> bool {
            let (mut cur, mut rc) = (obj, r);
            for _ in 0..MAX_HOPS {
                // The builtin prototypes are static layouts: `%Object.prototype%` is
                // a hop like any other, its list complete and its link null.
                if !walks_from(cur, rc) {
                    return false;
                }
                let next = match prototype_of(dir, cur, rc) {
                    Next::Object(p) => p,
                    Next::End => return true,
                    Next::Unknown => return false,
                };
                let w = (*next).parent_class_id;
                // An unwritten `%Object.prototype%` (its sealed layout's word, no
                // shadow bit) ends the chain and holds one accessor, `__proto__`,
                // which no family layout names.
                if w < crate::object::layout_desc::UNSHADOWED_WORD_END
                    && object_prototype(dir) == Some(next)
                {
                    return true;
                }
                let rn = record_of(dir, w);
                if rn.is_null() {
                    return false;
                }
                let t = if w & crate::object::overflow::SHADOW_BIT != 0 {
                    crate::object::overflow::table(next)
                } else {
                    std::ptr::null_mut()
                };
                if !if t.is_null() {
                    hop_admits_family(rn, d)
                } else {
                    overflow_admits_family(t, d)
                } {
                    return false;
                }
                cur = next;
                rc = rn;
            }
            false
        }

        /// [`chain_admits_family`]'s test of one hop's record `rn`.
        #[inline(always)]
        unsafe fn hop_admits_family(
            rn: *mut ShapeRecord,
            d: &crate::object::layout_desc::PerryLayoutDesc,
        ) -> bool {
            use crate::object::key_attrs::{
                keys_entry, ENTRY_ACCESSOR_MASK, ENTRY_NON_WRITABLE, ENTRY_PRIVATE,
                SUMMARY_BLOCKS_STORE, SUMMARY_PRIVATE,
            };
            if (*rn).summary() & (SUMMARY_BLOCKS_STORE | SUMMARY_PRIVATE) == 0 {
                return true;
            }
            let word = probe_word(rn);
            if word == 0 || word & PROBE_DECLINE != 0 {
                return false;
            }
            let table = (word & !PROBE_FLAGS) as *const KeyIndex;
            if !table.is_null() {
                let n = (*table).nblocks;
                if n == BLOCKS_UNKNOWN {
                    return false;
                }
                let blocks = KeyIndex::blocks(table);
                return (0..n as usize).all(|i| !desc_holds_hash(d, *blocks.add(i)));
            }
            // A short list the probe scans: its blocking entries by their words.
            let keys = (*rn).keys as usize as *const crate::array::ArrayHeader;
            let words = crate::array::array_elements_ptr(keys) as *const u64;
            for p in 0..(*rn).logical_key_count {
                let bits = *words.add(p as usize);
                if bits == TAG_HOLE
                    || keys_entry(keys, p)
                        & (ENTRY_ACCESSOR_MASK | ENTRY_NON_WRITABLE | ENTRY_PRIVATE)
                        == 0
                {
                    continue;
                }
                match key_word_hash(bits) {
                    Some(h) if !desc_holds_hash(d, h) => {}
                    _ => return false,
                }
            }
            true
        }

        /// [`hop_admits_family`] for a hop with an overflow (`t`), which holds every
        /// own key's attributes in its entries: its attribute summary, else its
        /// blocking entries' hashes against `d`'s index.
        #[cold]
        unsafe fn overflow_admits_family(
            t: *const crate::object::overflow::Table,
            d: &crate::object::layout_desc::PerryLayoutDesc,
        ) -> bool {
            use crate::object::key_attrs::{
                ENTRY_ACCESSOR_MASK, ENTRY_NON_WRITABLE, ENTRY_PRIVATE, SUMMARY_BLOCKS_STORE,
                SUMMARY_PRIVATE,
            };
            if (*t).summary & u32::from(SUMMARY_BLOCKS_STORE | SUMMARY_PRIVATE) == 0 {
                return true;
            }
            for i in 0..(*t).length {
                let key = *crate::object::overflow::key_at(t, i);
                if key == TAG_HOLE
                    || (*crate::object::overflow::attrs_at(t, i) as u8)
                        & (ENTRY_ACCESSOR_MASK | ENTRY_NON_WRITABLE | ENTRY_PRIVATE)
                        == 0
                {
                    continue;
                }
                match key_word_hash(key) {
                    Some(h) if !desc_holds_hash(d, h) => {}
                    _ => return false,
                }
            }
            true
        }

        /// Does descriptor `d` hold a key whose atom hash is `hash`? One probe of
        /// its immutable hash index. Equal hashes answer yes, so a hash collision
        /// declines a run, never admits a wrong one.
        #[inline(always)]
        unsafe fn desc_holds_hash(
            d: &crate::object::layout_desc::PerryLayoutDesc,
            hash: u64,
        ) -> bool {
            if d.index.is_null() {
                return true;
            }
            let mask = d.index_mask as u64;
            let mut at = hash & mask;
            for _ in 0..=mask {
                let pos = *d.index.add(at as usize);
                if pos == u32::MAX {
                    return false;
                }
                if pos < d.key_count && (*(*d.keys.add(pos as usize)).atom).hash == hash {
                    return true;
                }
                at = (at + 1) & mask;
            }
            true
        }

        return hole();
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return hole();
    }
    perry_probe_get(
        dir,
        (payload - perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
        key_bits,
        hash,
    )
}
