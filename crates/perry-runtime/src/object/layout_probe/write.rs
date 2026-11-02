use super::*;

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
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
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
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
        != 0
        || (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
        // A family object keeps its layout word for life: a key its layout
        // does not name joins its overflow (`layout_desc::family_store`).
        || crate::object::layout_desc::is_family_object(obj)
    {
        return false;
    }
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
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
    let dir = super::super::ordinary_dir_addr();
    let r = record_of(dir, LayoutWord((*obj).parent_class_id));
    if r.is_null() || !(*r).object_kind().is_ordinary_layout() {
        return None;
    }
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if !meta.is_null() && (*meta).overflow != 0 {
        return None;
    }
    if own_position(r, key_bits, hash) != Err(true) {
        return None;
    }
    chain_admits_add(dir, obj, r, key_bits, hash).then_some(true)
}

/// The property-atom hash of `__proto__` (FNV-1a, `key_bytes_hash`).
pub(super) const PROTO_KEY_HASH: u64 = {
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

/// One prototype hop `next` of [`chain_admits_add`]'s walk: `Err(answer)`
/// when `next`'s own layout (or overflow) decides the add (a plain writable
/// data property of the name is shadowed: admitted; an accessor or a
/// non-writable one intercepts), `Ok(record)` to continue at `next`'s
/// [[Prototype]]. GC leaf.
#[inline(always)]
unsafe fn hop_admits_add(
    dir: *const u8,
    next: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> Result<*mut ShapeRecord, bool> {
    let w = (*next).parent_class_id;
    // An unwritten `%Object.prototype%` ends the chain, and its one
    // accessor is `__proto__` (a key of that hash takes the walk).
    if hash != PROTO_KEY_HASH
        && w < crate::object::layout_desc::UNSHADOWED_WORD_END
        && object_prototype(dir) == Some(next)
    {
        return Err(true);
    }
    let rn = record_of(dir, LayoutWord(w));
    if rn.is_null() {
        return Err(false);
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
                return Err(false);
            }
            if value_at(next, rn, pos).to_bits() != TAG_HOLE {
                return Err(true);
            }
        }
        Err(false) => return Err(false),
        Err(true) => {}
    }
    if w & crate::object::overflow::SHADOW_BIT != 0 {
        // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(next as *mut _));
        if !meta.is_null() && (*meta).overflow != 0 {
            let t = (*meta).overflow as usize as *const crate::object::overflow::Table;
            if let Some(i) = crate::object::overflow::find_hashed(t, key_bits, hash) {
                return Err(crate::object::key_attrs::entry_is_plain_writable_data(
                    *crate::object::overflow::attrs_at(t, i) as u8,
                ));
            }
        }
    }
    Ok(rn)
}

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
        let rn = match hop_admits_add(dir, next, key_bits, hash) {
            Ok(rn) => rn,
            Err(answer) => return answer,
        };
        cur = next;
        rc = rn;
    }
    false
}

/// What a function object's own data put of `key` may do, from the layouts
/// alone (GPP4: a function's own keys are its bag's layout, which is its
/// keyed Function shape's key list).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FunctionPut {
    /// `key` is an own data property the put may overwrite in place.
    Overwrite,
    /// `key` is absent from the complete own list, and (for `[[Set]]`)
    /// nothing on the function's chain intercepts the add.
    Add,
    /// The layouts cannot say: the general put runs.
    Decline,
}

/// [`FunctionPut`] for the function whose bag is `bag` (non-null) and whose
/// [[Prototype]] is `link` (`TAG_NULL` or the prototype's pointer bits, read
/// from the function's shape), for the interned key `key`. `define` is
/// CreateDataProperty (the chain is never asked; an existing key must carry
/// default attributes), else `[[Set]]` (an existing key must be writable
/// data; an absent key needs the chain's admission, [`chain_admits_add`]
/// starting at `link`). GC leaf: reads layouts and slots only.
///
/// # Safety
/// `bag` is a live function bag; `key` a live interned heap string.
pub(crate) unsafe fn function_put_plan(
    bag: *const ObjectHeader,
    link: u64,
    key: *const crate::StringHeader,
    define: bool,
) -> FunctionPut {
    let Some((key_bits, hash)) = computed_key(STRING_TAG | key as u64) else {
        return FunctionPut::Decline;
    };
    let dir = super::super::ordinary_dir_addr();
    let w = (*bag).parent_class_id;
    // An overflow holds keys the layout does not list.
    if w & crate::object::overflow::SHADOW_BIT != 0 {
        return FunctionPut::Decline;
    }
    let r = record_of(dir, LayoutWord(w));
    if r.is_null() || !(*r).object_kind().is_ordinary_layout() || (*r).hole_count != 0 {
        return FunctionPut::Decline;
    }
    match own_position(r, key_bits, hash) {
        Ok(pos) => {
            let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
            let entry = crate::object::key_attrs::keys_entry(keys, pos);
            let admitted = if define {
                entry == 0
            } else {
                crate::object::key_attrs::entry_is_plain_writable_data(entry)
            };
            if admitted && value_at(bag, r, pos).to_bits() != TAG_HOLE {
                FunctionPut::Overwrite
            } else {
                FunctionPut::Decline
            }
        }
        Err(false) => FunctionPut::Decline,
        Err(true) if define || link_admits_add(dir, link, key_bits, hash) => FunctionPut::Add,
        Err(true) => FunctionPut::Decline,
    }
}

/// [`chain_admits_add`] for a receiver that is not an ordinary object (a
/// function), from its [[Prototype]] `link`: `TAG_NULL` ends the chain, an
/// ordinary object is the walk's first hop. GC leaf.
unsafe fn link_admits_add(dir: *const u8, link: u64, key_bits: u64, hash: u64) -> bool {
    if link == crate::value::TAG_NULL {
        return true;
    }
    if link & TAG_MASK != POINTER_TAG
        || ((link & POINTER_MASK) as usize) < perry_abi::RECEIVER_HANDLE_FLOOR
    {
        return false;
    }
    let next = (link & POINTER_MASK) as *const ObjectHeader;
    let header = crate::gc::header_from_trusted_user_ptr(next.cast());
    if (*header).obj_type != crate::gc::GC_TYPE_OBJECT
        || (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return false;
    }
    match hop_admits_add(dir, next, key_bits, hash) {
        Ok(rn) => chain_admits_add(dir, next, rn, key_bits, hash),
        Err(answer) => answer,
    }
}

/// May `[[Set]]` add the interned key `key` to a function whose
/// [[Prototype]] is `link` and which has no own key of that name
/// ([`link_admits_add`])? GC leaf.
///
/// # Safety
/// `key` is a live interned heap string; `link` is `TAG_NULL` or live.
pub(crate) unsafe fn function_link_admits_add(link: u64, key: *const crate::StringHeader) -> bool {
    let Some((key_bits, hash)) = computed_key(STRING_TAG | key as u64) else {
        return false;
    };
    link_admits_add(super::super::ordinary_dir_addr(), link, key_bits, hash)
}
