/// The hit path's proven own miss (its list proved the key absent): the
/// overflow, then the prototype chain. The record is read again from the
/// object, so the hit path passes four values and keeps no frame.
#[inline(never)]
unsafe fn absent_from<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let word = (*obj).parent_class_id;
    let r = record_of(dir, word);
    let probe = (*r).probe;
    if word & crate::object::overflow::SHADOW_BIT != 0 {
        return overflow_then_inherited::<ABSENT>(dir, obj, r, probe, key_bits, hash);
    }
    // A list with a key no compare can match (`PROBE_ATOMS` clear) proves
    // nothing by a miss.
    if probe & PROBE_ATOMS == 0 {
        return hole();
    }
    inherited_get::<ABSENT>(dir, obj, r, key_bits, hash)
}

/// The complete prototype walk of [`inherited_get`].
#[inline(never)]
unsafe fn inherited_walk<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> f64 {
    match walk_step::<ABSENT>(dir, obj, r) {
        Ok(next) => inherited_walk_at::<ABSENT>(dir, next, key_bits, hash),
        Err(v) => v,
    }
}

/// The next hop of the complete walk from `cur` (record `rc`), or the walk's
/// answer when it ends at `cur`.
#[inline(always)]
unsafe fn walk_step<const ABSENT: bool>(
    dir: *const u8,
    cur: *const ObjectHeader,
    rc: *const ShapeRecord,
) -> Result<*const ObjectHeader, f64> {
    let absent = || {
        if ABSENT {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            hole()
        }
    };
    if !walks_from(cur, rc) {
        // A runtime-born record (unmarked, its keys all in its list and
        // overflow) whose chain ends here proves absence.
        if (*rc).object_kind() == super::ShapeObjectKind::OrdinaryUnmarked
            && ((*cur).meta.is_null() || (*(*cur).meta).flags & READ_BEYOND_LIST == 0)
            && matches!(prototype_of(dir, cur, rc), Next::End)
        {
            return Err(absent());
        }
        return Err(hole());
    }
    match prototype_of(dir, cur, rc) {
        Next::Object(p) => Ok(p),
        Next::End => Err(absent()),
        Next::Unknown => Err(hole()),
    }
}

/// The complete walk from hop `next` on: the prototype a caller's walk has
/// already reached, so its link is not derived again (the fast chain hands
/// over the hop it could not answer).
#[inline(never)]
unsafe fn inherited_walk_at<const ABSENT: bool>(
    dir: *const u8,
    mut next: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    for _ in 0..MAX_HOPS {
        let w = (*next).parent_class_id;
        let rn = record_of(dir, w);
        if rn.is_null() {
            return hole();
        }
        match own_position(rn, key_bits, hash) {
            Ok(pos) => {
                let v = value_at(next, rn, pos);
                // A hole slot of a plain data key: absent on this hop.
                if v.to_bits() != TAG_HOLE || !plain_data_at(rn, pos) {
                    return v;
                }
            }
            Err(false) => return hole(),
            Err(true) => {}
        }
        if w & crate::object::overflow::SHADOW_BIT != 0 {
            if let Some(v) = overflow_get(next, key_bits, hash) {
                return v;
            }
        }
        // A sealed builtin prototype not yet on a static layout may install
        // methods outside its list, so its miss proves nothing.
        match (*rn).object_kind() {
            super::ShapeObjectKind::Ordinary => {}
            kind if listed(next, kind) => {}
            _ => return hole(),
        }
        next = match walk_step::<ABSENT>(dir, next, rn) {
            Ok(p) => p,
            Err(v) => return v,
        };
    }
    hole()
}

/// The accessor `[[Get]]` of `key` on `obj` would call, when the probe's
/// walk finds the key as an accessor of `obj` or of the first prototype that
/// holds it (every object before it proving the key absent). `None` for
/// everything else, which the caller's complete operation answers. Not a
/// leaf contract of its own: it reads exactly what [`perry_probe_get`] reads.
///
/// # Safety
/// `obj` is a live object above the handle band; `key` a live string.
pub(crate) unsafe fn probe_accessor(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<crate::object::accessor_pair::Accessor> {
    if key.is_null() || (obj as usize) < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    let atom = if crate::string::is_property_atom(key) {
        key
    } else {
        crate::string::existing_property_atom(key)?
    };
    probe_accessor_word(
        obj,
        STRING_TAG | atom as u64,
        crate::string::property_atom_hash(atom),
    )
}

/// [`probe_accessor`] for a key word as a site passes it: a property atom
/// (STRING-tagged) or a symbol, and its hash.
///
/// # Safety
/// `obj` is a live object above the handle band.
pub(crate) unsafe fn probe_accessor_word(
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> Option<crate::object::accessor_pair::Accessor> {
    if (obj as usize) < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    let dir = super::ordinary_dir_addr();
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let mut cur = obj;
    let mut rc = record_of(dir, (*cur).parent_class_id) as *const ShapeRecord;
    if rc.is_null() || !(*rc).present() {
        return None;
    }
    for hop in 0..=MAX_HOPS {
        if hop > 0
            || (*rc).object_kind() == super::ShapeObjectKind::Ordinary
            || listed(cur, (*rc).object_kind())
        {
            match own_entry(rc as *mut ShapeRecord, key_bits, hash) {
                Ok((pos, pair)) => return accessor_in(cur, rc, pos, pair),
                Err(false) => return None,
                Err(true) => {}
            }
            if (*cur).parent_class_id & crate::object::overflow::SHADOW_BIT != 0
                && !matches!(overflow_get(cur, key_bits, hash), None)
            {
                return None;
            }
        } else {
            return None;
        }
        if !walks_from(cur, rc) {
            return None;
        }
        let next = match prototype_of(dir, cur, rc) {
            Next::Object(p) => p,
            _ => return None,
        };
        let rn = record_of(dir, (*next).parent_class_id) as *const ShapeRecord;
        if rn.is_null() || !(*rn).present() {
            return None;
        }
        match (*rn).object_kind() {
            super::ShapeObjectKind::Ordinary => {}
            kind if listed(next, kind) => {}
            _ => {
                // A builtin prototype's list holds what the program defined
                // on it; an accessor found there is that accessor.
                return match own_entry(rn as *mut ShapeRecord, key_bits, hash) {
                    Ok((pos, pair)) => accessor_in(next, rn, pos, pair),
                    Err(_) => None,
                };
            }
        }
        cur = next;
        rc = rn;
    }
    None
}

/// The accessor at key position `pos` of `obj` (record `r`): one load of
/// inline slot `pair` when the record's index names it ([`ACCESSOR_ENTRY`]),
/// else ([`NO_LOAD`]) [`accessor_at`].
#[inline(always)]
unsafe fn accessor_in(
    obj: *const ObjectHeader,
    r: *const ShapeRecord,
    pos: u32,
    pair: u32,
) -> Option<crate::object::accessor_pair::Accessor> {
    if pair == NO_LOAD {
        return accessor_at(obj, r, pos);
    }
    let bits = *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + pair as usize * 8)
        as *const u64);
    Some(crate::object::accessor_pair::pair_of_value(bits).unwrap_or_default())
}

/// The accessor at key position `pos` of `obj` (record `r`), if that key is
/// an accessor.
#[inline(never)]
unsafe fn accessor_at(
    obj: *const ObjectHeader,
    r: *const ShapeRecord,
    pos: u32,
) -> Option<crate::object::accessor_pair::Accessor> {
    if !positional_layout(&*r) || (*r).summary() & crate::object::key_attrs::SUMMARY_ACCESSOR == 0 {
        return None;
    }
    let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
    if crate::object::key_attrs::keys_entry(keys, pos)
        & crate::object::key_attrs::ENTRY_ACCESSOR_MASK
        == 0
    {
        return None;
    }
    let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
    let bits = if slot < (*r).live_inline_slot_count {
        *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const u64)
    } else {
        crate::object::spill::spill_get_present(obj as usize, slot as usize)?
    };
    Some(crate::object::accessor_pair::pair_of_value(bits).unwrap_or_default())
}

/// The value of `obj`'s own PRIVATE entry `key_bits` (a property atom,
/// `hash` its hash), read by its layout as the probe reads a public key;
/// `None` when the layout does not list it (or cannot answer). A runtime
/// intrinsic's private field is the only reader: a public read never sees a
/// private entry (`value_at`).
///
/// # Safety
/// `obj` is a live object above the handle band.
pub(crate) unsafe fn own_private_value(
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> Option<f64> {
    let dir = super::ordinary_dir_addr();
    let r = record_of(dir, (*obj).parent_class_id);
    if r.is_null() || !(*r).present() || !positional_layout(&*r) {
        return None;
    }
    let pos = own_position(r, key_bits, hash).ok()?;
    let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
    if crate::object::key_attrs::keys_entry(keys, pos) & crate::object::key_attrs::ENTRY_PRIVATE
        == 0
    {
        return None;
    }
    let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
    let bits = if slot < (*r).live_inline_slot_count {
        *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const u64)
    } else {
        crate::object::spill::spill_get_present(obj as usize, slot as usize)?
    };
    (bits != TAG_HOLE).then_some(f64::from_bits(bits))
}

/// A class method site's own-override check (`this.m = fn` shadowing the
/// class method, perry-codegen `emit_own_method_override_check`): the own
/// DATA value of `key_bits` (the site's property atom, `hash` its hash) on
/// `recv`, or `undefined` when `recv` has no own data property of that name
/// (absent, a hole, an accessor). The receiver's layout answers it with the
/// probe's own lookup (one index probe, atoms compared by identity); what the
/// layout cannot answer takes the by-bytes own scan
/// (`js_object_get_own_field_or_undef`), which this answers identically.
///
/// GC leaf, like the by-bytes scan.
///
/// # Safety
/// `dir` is this agent's ordinary directory or `PERRY_EMPTY_SHAPE_DIR`;
/// `key_bits` a STRING-tagged property atom; `recv` any value.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_own_method(
    dir: *const u8,
    recv: f64,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let bits = recv.to_bits();
    let payload = (bits & POINTER_MASK) as usize;
    if bits & TAG_MASK == POINTER_TAG && payload >= perry_abi::RECEIVER_HANDLE_FLOOR {
        let h = crate::gc::header_from_trusted_user_ptr(payload as *const u8);
        let obj = payload as *const ObjectHeader;
        if (*h).obj_type == crate::gc::GC_TYPE_OBJECT
            && (*h).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            && (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT == 0
        {
            let r = record_of(dir, (*obj).parent_class_id);
            if !r.is_null() && (*r).present() {
                match own_position(r, key_bits, hash) {
                    Ok(pos) => {
                        let v = value_at(obj, r, pos);
                        return if v.to_bits() == TAG_HOLE {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        } else {
                            v
                        };
                    }
                    Err(true) => return f64::from_bits(crate::value::TAG_UNDEFINED),
                    Err(false) => {}
                }
            }
        }
    }
    own_method_by_bytes(recv, key_bits)
}

/// [`perry_probe_own_method`]'s by-bytes scan of the key's text.
#[cold]
#[inline(never)]
unsafe fn own_method_by_bytes(recv: f64, key_bits: u64) -> f64 {
    let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
    if key_bits & TAG_MASK != STRING_TAG || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::object::js_object_get_own_field_or_undef(
        recv,
        crate::string::string_data(key),
        (*key).byte_len as usize,
    )
}

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
    let dir = super::ordinary_dir_addr();
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
unsafe fn class_prototype_object(obj: *const ObjectHeader) -> bool {
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
    if obj_bits & TAG_MASK != POINTER_TAG {
        return 0;
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return 0;
    }
    let obj = payload as *mut ObjectHeader;
    let r = record_of(dir, (*obj).parent_class_id);
    if r.is_null() || (*r).object_kind() != super::ShapeObjectKind::Ordinary {
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
                return i32::from(probe_add(dir, obj, r, key_bits, hash, bits));
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
unsafe fn hole_fill_admitted(
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
unsafe fn probe_add(
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
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
        != 0
        || (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
        // A family object keeps its layout word for life: a key its layout
        // does not name joins its overflow (`layout_desc::family_store`).
        || crate::object::layout_desc::is_family_object(obj)
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
    // The shadow bit (`shadow.rs`) does not refuse the add. The caller
    // proved the object has no overflow (the bit's other meaning), so its
    // layout lists every own key; the add's publication carries the bit and
    // sets it when the new key is a shadow name
    // (`shadow::publish_key_add_word`). A class-id-0 object takes the bit at
    // its first key that names any class member of the program: in a large
    // program that is nearly every zod schema, after `check` or `parse`.
    // An own descriptor on another key does not touch this add: the probe
    // proved `key` absent from a complete own list, and an ordinary object's
    // attributes live in that list (`key_attrs`), so no own descriptor covers
    // it. `OBJ_FLAG_HAS_DESCRIPTORS` is the object-wide, set-only flag; the
    // per-key answer is the key list (zod v4 defines `_zod` non-enumerable,
    // then adds ~60 methods by assignment). A typed array keeps its synthetic
    // attributes elsewhere, and an arguments object's are born with its
    // layout, so both keep the full `[[Set]]`.
    if (*gc)._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        && ((*gc)._reserved & crate::gc::OBJ_FLAG_ARGUMENTS != 0
            || !crate::object::key_attrs::attrs_live_in_keys_of_bits(POINTER_TAG | obj as u64))
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
    let dir = super::ordinary_dir_addr();
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
unsafe fn chain_admits_add(
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
            // The realm-default link before `%Object.prototype%`'s address is
            // memoized (the probe's cell): the generic store's authority for
            // that link answers (`descriptor_state::layout_store_may_intercept`
            // on a realm-default record), without its per-key read.
            Next::Unknown => {
                return hash != PROTO_KEY_HASH
                    && realm_default_link(cur, rc)
                    && !crate::object::descriptor_state::object_proto_may_intercept_any();
            }
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
