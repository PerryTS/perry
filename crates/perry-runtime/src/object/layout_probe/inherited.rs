use super::*;

/// May the probe continue from `obj` (record `r`) to its prototype? Only an
/// object whose own keys are exactly its layout's list and its overflow: an
/// ordinary object, or an unmarked or callable one that [`listed`] admits; no
/// native-alias, namespace or numeric-proof kind, no exotic read receiver.
#[inline(always)]
pub(super) unsafe fn walks_from(obj: *const ObjectHeader, r: *const ShapeRecord) -> bool {
    match (*r).object_kind() {
        super::super::ShapeObjectKind::Ordinary => {
            // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
            meta.is_null()
                || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER == 0
        }
        kind => listed(obj, kind),
    }
}

/// Meta flags under which an object's reads answer beyond its list and
/// overflow: an exotic read receiver, or a native-this alias whose misses
/// forward to the native object (`native_this_alias`). An admitted receiver
/// moves to the `OrdinaryNativeAlias` kind when aliased; an unmarked one keeps
/// its kind, so for it the flag on its own meta is the fact.
pub(super) const READ_BEYOND_LIST: u64 = crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
    | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS;

/// Are the own keys of `obj`, an object of the unmarked or callable ordinary
/// `kind`, exactly its list and overflow? An unmarked object qualifies (a
/// runtime-born record, a user prototype, a sealed intrinsic of any family:
/// the builtin and runtime prototypes born on complete static layouts) unless
/// it is an exotic read receiver or a typed-array prototype; a callable one
/// only as such a prototype on its static layout (`%Function.prototype%`,
/// [`crate::object::builtin_proto::on_static_layout`]).
#[inline(always)]
pub(super) unsafe fn listed(obj: *const ObjectHeader, kind: super::super::ShapeObjectKind) -> bool {
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    let flags = if meta.is_null() { 0 } else { (*meta).flags };
    if flags & crate::object::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC != 0 {
        return flags & READ_BEYOND_LIST == 0
            && match kind {
                super::super::ShapeObjectKind::OrdinaryUnmarked => true,
                super::super::ShapeObjectKind::CallableOrdinary => {
                    crate::object::builtin_proto::on_static_layout(flags)
                }
                _ => false,
            };
    }
    if kind != super::super::ShapeObjectKind::OrdinaryUnmarked || flags & READ_BEYOND_LIST != 0 {
        return false;
    }
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    (*header)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO == 0
}

/// Where a prototype walk goes from an object.
pub(super) enum Next {
    /// The [[Prototype]] object.
    Object(*const ObjectHeader),
    /// The chain ends here ([[Prototype]] is null).
    End,
    /// A link the probe does not follow.
    Unknown,
}

/// The agent's `Object.prototype`, read through its shape directory `dir`
/// (`BandDir.proto_cell`): `None` before it is resolved.
#[inline]
pub(super) unsafe fn object_prototype(dir: *const u8) -> Option<*const ObjectHeader> {
    let cell = *(dir.add(FLAT_BASE_OFFSET + 16) as *const *const usize);
    if cell.is_null() {
        return None;
    }
    let p = *cell;
    if p == usize::MAX || p < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    // The cell names the realm's `%Object.prototype%`, an ordinary heap
    // object, so its header is read directly. The collector rewrites the
    // cell; a cell naming a forwarded copy is not followed (the slow path
    // heals it).
    let h = crate::gc::header_from_trusted_user_ptr(p as *const u8);
    ((*h).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0).then_some(p as *const ObjectHeader)
}

/// `obj`'s [[Prototype]] (`r` is its record), by the rule the runtime's own
/// shape link reads (`object_ops::prototype::shape_link_prototype` and
/// `shapes::object_prototype_word`): a recorded prototype first, then the
/// shape's identity: null, the realm default `%Object.prototype%`, a class
/// declaration prototype (the identity's word), or a linked identity's word.
pub(super) unsafe fn prototype_of(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *const ShapeRecord,
) -> Next {
    let header = crate::gc::header_from_trusted_user_ptr(obj.cast());
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    if (*header)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        && (crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _)).is_null()
            || (*crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _))).prototype == 0)
        && (*r).proto_id == super::super::PROTO_ID_DEFAULT
    {
        return Next::Unknown;
    }
    link_of(dir, obj, r)
}

/// Is `cur`'s [[Prototype]] the realm default `%Object.prototype%` by
/// [`link_of`]'s rule (no recorded prototype, a realm-default identity, not a
/// typed-array prototype, no exotic read receiver or weak-collection brand)?
#[inline]
pub(super) unsafe fn realm_default_link(cur: *const ObjectHeader, rc: *const ShapeRecord) -> bool {
    let header = crate::gc::header_from_trusted_user_ptr(cur.cast());
    if (*header)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
        return false;
    }
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _));
    if !meta.is_null()
        && ((*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0
            || (*meta).prototype != 0)
    {
        return false;
    }
    (*rc).weak_collection_brand().is_none() && (*rc).proto_id == super::super::PROTO_ID_DEFAULT
}

/// The [[Prototype]] link of `cur` (record `rc`), an object whose own keys
/// [`walks_from`] admits, so not a typed-array prototype: its recorded
/// prototype, else its layout's identity (null, the realm default
/// `%Object.prototype%`, a class identity's word, a linked identity's word).
#[inline(always)]
pub(super) unsafe fn link_of(
    dir: *const u8,
    cur: *const ObjectHeader,
    rc: *const ShapeRecord,
) -> Next {
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _));
    let mut bits = 0u64;
    if !meta.is_null() {
        if (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0 {
            return Next::Unknown;
        }
        bits = (*meta).prototype;
    }
    if bits == 0 {
        if (*rc).weak_collection_brand().is_some() {
            return Next::Unknown;
        }
        let pid = (*rc).proto_id;
        if pid == super::super::PROTO_ID_DEFAULT {
            return match object_prototype(dir) {
                Some(p) if p != cur => Next::Object(p),
                _ => Next::Unknown,
            };
        }
        if pid == super::super::PROTO_ID_NULL {
            return Next::End;
        }
        bits = if pid == super::super::PROTO_ID_RECORD_HELD {
            record_held_link(cur)
        } else if (super::super::PROTO_ID_CLASS..super::super::PROTO_ID_MIXED).contains(&pid) {
            let word_dir = *(dir.add(FLAT_BASE_OFFSET + 24) as *const *const u8);
            if word_dir.is_null() {
                return Next::Unknown;
            }
            match super::super::shapes_prototype::identity_word_slot_in(word_dir, pid) {
                Some(slot) => *slot,
                None => return Next::Unknown,
            }
        } else {
            (*rc).prototype_word()
        };
    }
    if bits == TAG_NULL {
        return Next::End;
    }
    if bits & TAG_MASK != POINTER_TAG
        || ((bits & POINTER_MASK) as usize) < perry_abi::RECEIVER_HANDLE_FLOOR
    {
        return Next::Unknown;
    }
    Next::Object((bits & POINTER_MASK) as *const ObjectHeader)
}

/// A generator instance's link: its record holds it. Out of line, so the
/// record read stays off every other receiver's hop.
#[inline(never)]
pub(super) unsafe fn record_held_link(cur: *const ObjectHeader) -> u64 {
    crate::object::generator_record_prototype_word(cur)
}

/// The first prototype of `obj` that holds `key_bits`, if every object
/// before it proves the key absent, and that holder's data value; with
/// `ABSENT`, `undefined` when the whole chain proves the key absent.
///
/// `obj`'s own list and overflow have proved the key absent; the chain is
/// read by [`chain_from`].
#[inline(never)]
pub(super) unsafe fn inherited_get<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *mut ShapeRecord,
    key_bits: u64,
    hash: u64,
) -> f64 {
    // `cur` is an object the walk may continue from (`walks_from`): the
    // receiver is checked here, each prototype before it becomes `cur`.
    if (*r).object_kind() != super::super::ShapeObjectKind::Ordinary {
        return inherited_walk::<ABSENT>(dir, obj, r, key_bits, hash);
    }
    match link_of(dir, obj, r) {
        Next::Object(next) => chain_from::<ABSENT>(dir, obj, next, key_bits, hash),
        Next::End => chain_absent::<ABSENT>(),
        Next::Unknown => hole(),
    }
}

/// The chain from `next`, the [[Prototype]] of `cur` (an object whose own
/// list and overflow proved `key_bits` absent; null for an exotic receiver),
/// read in one frame-light loop with each hop's work the layout's
/// own facts and the object's own words:
///
/// * the object's layout word, record and key index, the same probe the own
///   hit makes; a hit reads the holder's slot (`hit_value`);
/// * a miss continues while the object's own keys are exactly its list (no
///   shadow bit; [`walks_from`]), through its link ([`link_of`]), and ends
///   where a link is null. The builtin prototypes are static layouts
///   (`runtime_layouts::builtin_prototypes`), so `%Object.prototype%`'s miss
///   is absence like any other.
///
/// Every other case (an overflow on a prototype, a TEXT key entry, a record
/// not yet derived) continues in the complete walk ([`inherited_walk_at`])
/// at that hop, which answers it with the same rules; with no `cur` it
/// declines.
#[inline(always)]
pub(super) unsafe fn chain_from<const ABSENT: bool>(
    dir: *const u8,
    mut cur: *const ObjectHeader,
    mut next: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    // The complete walk continues at the hop this chain could not answer:
    // `next`, the link of `cur` the chain already derived.
    let walk = |cur: *const ObjectHeader, next: *const ObjectHeader| {
        if cur.is_null() {
            hole()
        } else {
            inherited_walk_at::<ABSENT>(dir, next, key_bits, hash)
        }
    };
    for _ in 0..MAX_HOPS {
        let w = (*next).parent_class_id;
        let rn = record_of(dir, LayoutWord(w));
        if rn.is_null() {
            return hole();
        }
        // A record's probe word is derived on its first probe.
        let pw = probe_word(rn);
        if pw & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS) != PROBE_READY | PROBE_ATOMS {
            return walk(cur, next);
        }
        match list_position(rn, pw, key_bits, hash) {
            Ok((pos, load)) => {
                let v = hit_value(next, rn, pos, load);
                if v.to_bits() != TAG_HOLE {
                    return v;
                }
                // An accessor or a private entry declines (the slow entry
                // calls a getter); a hole slot is absent on this hop, and the
                // complete walk continues past it.
                if load == NO_LOAD && attributed_at(rn, pos) {
                    return hole();
                }
                return walk(cur, next);
            }
            Err(true) => {}
            Err(false) => return walk(cur, next),
        }
        if w & crate::object::overflow::SHADOW_BIT != 0 {
            return walk(cur, next);
        }
        // A sealed builtin prototype not yet on a static layout may install
        // methods outside its list, so its miss proves nothing.
        match (*rn).object_kind() {
            super::super::ShapeObjectKind::Ordinary => {}
            kind if listed(next, kind) => {}
            _ => return hole(),
        }
        match link_of(dir, next, rn) {
            Next::Object(p) => {
                cur = next;
                next = p;
            }
            Next::End => return chain_absent::<ABSENT>(),
            Next::Unknown => return hole(),
        }
    }
    hole()
}

/// The chain proved the key absent.
#[inline(always)]
pub(super) fn chain_absent<const ABSENT: bool>() -> f64 {
    if ABSENT {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        hole()
    }
}

/// Position of `key_bits` in record `r`'s derived list (`pw` its ready probe
/// word with `PROBE_ATOMS`) and whether a hit there is one load
/// ([`hit_value`]'s slot): `Ok((pos, load))`, `Err(true)` for proven absence,
/// `Err(false)` for a TEXT entry the complete walk compares by text.
#[inline(always)]
pub(super) unsafe fn list_position(
    r: *const ShapeRecord,
    pw: u64,
    key_bits: u64,
    hash: u64,
) -> Result<(u32, u32), bool> {
    let count = (*r).logical_key_count;
    if count == 0 {
        return Err(true);
    }
    let list = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
    let table = (pw & !PROBE_FLAGS) as *const KeyIndex;
    if table.is_null() {
        let mut i = 0;
        while i < count {
            if *list.add(i as usize) == key_bits {
                return Ok((
                    i,
                    if i < (*r).position_bound_raw() {
                        i
                    } else {
                        NO_LOAD
                    },
                ));
            }
            i += 1;
        }
        return Err(true);
    }
    let mask = (*table).mask as usize;
    let slots = KeyIndex::buckets(table);
    let mut b = hash as usize & mask;
    loop {
        let pos = *slots.add(b);
        if pos == u32::MAX {
            return Err(true);
        }
        let i = pos & POS_BITS;
        if pos & TEXT_ENTRY != 0 {
            // Only a key of the text's hash can be its text, which the
            // complete walk compares.
            if KeyIndex::word(table, i) != hash {
                b = (b + 1) & mask;
                continue;
            }
            return Err(false);
        }
        let wd = if pos & ALIAS_ENTRY != 0 {
            KeyIndex::word(table, i)
        } else {
            *list.add(i as usize)
        };
        if wd == key_bits {
            return Ok((i, entry_load(table, pos, i)));
        }
        b = (b + 1) & mask;
    }
}

/// The hit path's proven own miss (its list proved the key absent): the
/// overflow, then the prototype chain. The record is read again from the
/// object, so the hit path passes four values and keeps no frame.
#[inline(never)]
pub(super) unsafe fn absent_from<const ABSENT: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    let word = (*obj).parent_class_id;
    let r = record_of(dir, LayoutWord(word));
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
pub(super) unsafe fn inherited_walk<const ABSENT: bool>(
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
pub(super) unsafe fn walk_step<const ABSENT: bool>(
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
        // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
        // A runtime-born record (unmarked, its keys all in its list and
        // overflow) whose chain ends here proves absence.
        if (*rc).object_kind() == super::super::ShapeObjectKind::OrdinaryUnmarked
            && (crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _)).is_null()
                || (*crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _))).flags
                    & READ_BEYOND_LIST
                    == 0)
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
pub(super) unsafe fn inherited_walk_at<const ABSENT: bool>(
    dir: *const u8,
    mut next: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    for _ in 0..MAX_HOPS {
        let w = (*next).parent_class_id;
        let rn = record_of(dir, LayoutWord(w));
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
            super::super::ShapeObjectKind::Ordinary => {}
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
