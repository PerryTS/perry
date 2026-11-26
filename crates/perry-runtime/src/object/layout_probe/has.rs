//! The probe's presence mode: `in`, `Object.hasOwn`, `hasOwnProperty`.
use super::*;

/// Whether `probe_has` must leave a layout accessor entry of `cur` to the
/// overflow probe: only an object with an overflow keeps its attributes (and
/// a deleted accessor's tombstone) there. The shadow bit's absence proves no
/// overflow (`LAYOUT_SHADOW_BIT`); a shadowed object without a table still
/// answers from the layout. Out of line so the data-entry path of
/// `probe_has` keeps its register shape.
#[cold]
#[inline(never)]
unsafe fn accessor_defers_to_overflow(cur: *const ObjectHeader) -> bool {
    (*cur).parent_class_id & crate::object::overflow::SHADOW_BIT != 0
        && crate::object::overflow::has_overflow(cur)
}

/// The probe's presence mode (`in`, `Object.hasOwn`,
/// `hasOwnProperty`): `1` when `key` is present on the object at `obj_bits`
/// (own, or along its prototype chain unless `own_only`), `0` when the
/// layouts prove it absent, `-1` when the probe cannot answer (an exotic
/// receiver, an index-like or uninterned key, a builtin prototype's unlisted
/// names, an unknown prototype). A hole slot is an absent key (the key view);
/// an accessor is present. GC leaf.
///
/// Own-only presence and chain presence are two instances
/// ([`probe_has_impl`]): every caller passes a literal, so this folds to one
/// call, and the own-only instance carries no prototype hop.
///
/// # Safety
/// `obj_bits` and `key_bits` any NaN-boxed values.
#[inline(always)]
pub(crate) unsafe fn probe_has(obj_bits: u64, key_bits: u64, own_only: bool) -> i32 {
    if own_only {
        probe_has_impl::<true>(obj_bits, key_bits)
    } else {
        probe_has_impl::<false>(obj_bits, key_bits)
    }
}

/// [`probe_has`] for one `OWN_ONLY`.
unsafe fn probe_has_impl<const OWN_ONLY: bool>(obj_bits: u64, key_bits: u64) -> i32 {
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
    let dir = super::super::ordinary_dir_addr();
    // The keyed read's own-slot arm, asked for presence.
    if exact && computed::own_slot_holds(dir, payload as *const ObjectHeader, key) {
        return 1;
    }
    probe_has_walk_in::<OWN_ONLY>(dir, payload, key, hash, exact)
}

/// [`probe_has`]'s presence walk for the resolved key `key` (its hash
/// `hash`; `exact` when it is the key's atom or symbol, so that a miss
/// proves absence) from the object at `payload`. Inlined into its two
/// callers ([`probe_has`] and the `in` operator's miss arm), so a miss pays
/// one frame, not two.
#[inline(always)]
pub(crate) unsafe fn probe_has_walk(
    dir: *const u8,
    payload: usize,
    key: u64,
    hash: u64,
    exact: bool,
    own_only: bool,
) -> i32 {
    if own_only {
        probe_has_walk_in::<true>(dir, payload, key, hash, exact)
    } else {
        probe_has_walk_in::<false>(dir, payload, key, hash, exact)
    }
}

/// [`probe_has_walk`] for one `OWN_ONLY`: the own-only instance answers at
/// the receiver and has no prototype hop, so none of the chain arms (the
/// link, the descriptor lookup) lives in its code.
#[inline(always)]
unsafe fn probe_has_walk_in<const OWN_ONLY: bool>(
    dir: *const u8,
    payload: usize,
    key: u64,
    hash: u64,
    exact: bool,
) -> i32 {
    let absent = if exact { 0 } else { -1 };
    let mut cur = payload as *const ObjectHeader;
    for _ in 0..MAX_HOPS {
        if (*cur).class_id == crate::object::deferred_namespace::DEFERRED_NAMESPACE_CLASS_ID {
            return -1;
        }
        let w = (*cur).parent_class_id;
        let r = record_of(dir, LayoutWord(w));
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
                // The attributes of an object with an overflow live in its
                // entries, not in its layout (`overflow::set_entry_attrs`),
                // and a deleted accessor leaves only its tombstone there: the
                // layout's accessor entry answers only when there is no
                // overflow, else the overflow probe below does.
                if entry & crate::object::key_attrs::ENTRY_ACCESSOR_MASK != 0 {
                    if !accessor_defers_to_overflow(cur) {
                        return 1;
                    }
                } else {
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
            // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _));
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
        if OWN_ONLY {
            // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
            // An Array subclass's elements store holds its own index keys and
            // `length`, which no key list names.
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _));
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
        // ChainDesc: an exact string key's presence at and above a marked
        // prototype is one lookup in its descriptor. A symbol keeps the walk
        // (a class prototype's declared symbol methods are not in its list).
        if exact && key & TAG_MASK == STRING_TAG {
            let desc = super::chain_glue::desc_of(cur);
            if !desc.is_null() {
                return super::chain_glue::has(dir, cur, desc, key, hash, absent);
            }
        }
    }
    -1
}

/// [`probe_has_walk`] from the prototype hop `from` (a descriptor's holder or
/// boundary), out of line for [`super::chain_glue::has`].
#[inline(never)]
pub(super) unsafe fn probe_has_walk_from(
    dir: *const u8,
    from: *const ObjectHeader,
    key: u64,
    hash: u64,
) -> i32 {
    probe_has_walk_in::<false>(dir, from as usize, key, hash, true)
}

/// Is `obj` (class id nonzero) a class declaration's or a class
/// evaluation's prototype object?
#[cold]
#[optimize(size)]
#[inline(never)]
unsafe fn class_prototype_object(obj: *const ObjectHeader) -> bool {
    crate::object::class_value::class_decl_prototype_link((*obj).class_id) as usize == obj as usize
        || crate::object::field_get_set::class_evaluation_prototype_class_id(obj as usize).is_some()
}
