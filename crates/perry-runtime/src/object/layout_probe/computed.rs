use super::*;

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
        /// * from `pos` on, no key is present after a hole (a present key's store
        ///   overwrites it; a hole's is an add with every later key a hole, so in
        ///   key order, and so is every later add of the run); `order_known`
        ///   (nonzero) when codegen proves it: no code since the birth could have
        ///   stored a key;
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
            // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
            let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
            if !meta.is_null()
                && ((*meta).overflow != 0
                    || (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE != 0)
            {
                note_run(4);
                return 0;
            }
            // The order: from `pos` on, no key is present after a hole. The run's
            // store of a present key overwrites it; of a hole, adds it while
            // every later key is a hole, in key order. A run that starts before
            // any code since the birth could run (`order_known`, the
            // constructors' inert prologue) knows it.
            if order_known == 0 {
                let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
                let mut hole = false;
                if keys[pos as usize..].iter().any(|k| {
                    let present = *crate::object::header::object_slot_at(
                        obj as *mut ObjectHeader,
                        u32::from(k.off),
                    ) != TAG_HOLE;
                    let after_hole = present && hole;
                    hole |= !present;
                    after_hole
                }) {
                    note_run(5);
                    if crate::object::layout_desc::family_diag_pairs_on() {
                        crate::object::layout_desc::note_out_of_order_pair(obj, pos);
                    }
                    return 0;
                }
            }
            let r = record_of(dir, LayoutWord(word));
            if r.is_null() {
                note_run(7);
                return 0;
            }
            let admits = chain_admits_family(dir, obj, r, d);
            note_run(if admits { 0 } else { 6 });
            i32::from(admits)
        }

        /// `PERRY_PROBE_VERIFY`: the witness of a compiled family slot access
        /// ([`verify::slot`]), emitted only by a compile with the knob set.
        /// GC leaf.
        ///
        /// # Safety
        /// `obj_bits` is the address (or NaN-boxed pointer) of a live object
        /// whose layout word the site just tested.
        #[no_mangle]
        #[inline(never)]
        pub unsafe extern "C" fn perry_family_slot_verify(obj_bits: u64, key_bits: u64, slot: u32) {
            verify::slot(obj_bits, key_bits, slot);
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
                let rn = record_of(dir, LayoutWord(w));
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

/// The canonical word and hash of a computed property key, as a site key
/// would carry them: a property atom or a symbol as it is; a short or plain
/// string as its text's property atom when one exists. `None` for anything
/// else (a number, an object whose ToPropertyKey runs user code, a text no
/// atom names).
#[inline(always)]
pub(super) unsafe fn computed_key(key_bits: u64) -> Option<(u64, u64)> {
    let tag = key_bits & TAG_MASK;
    if tag == STRING_TAG {
        let s = (key_bits & POINTER_MASK) as *const crate::StringHeader;
        if s.is_null() {
            return None;
        }
        if crate::string::is_property_atom(s) {
            let text =
                std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
            if index_like(text) {
                return None;
            }
            return Some((key_bits, crate::string::property_atom_hash(s)));
        }
        // A shared string that was a key before carries its key word (its
        // text's folded hash; never written for an index-like text): the
        // atom index is probed from the word, no bytes are hashed.
        let word = (*s).refcount;
        let (a, h) = if word >= crate::string::KEY_WORD_MIN {
            crate::string::key_word_atom(s, word)
        } else {
            heap_key_first(s)
        };
        if a.is_null() {
            return None;
        }
        return Some((STRING_TAG | a as u64, h));
    }
    if tag == crate::value::SHORT_STRING_TAG {
        // A short string's word is its text: hash it from the word (the
        // FNV-1a of `key_bytes_hash`) and find its atom by a word compare.
        let len = ((key_bits & crate::value::SHORT_STRING_LEN_MASK)
            >> crate::value::SHORT_STRING_LEN_SHIFT) as u32;
        if len == 0 || len as usize > crate::value::SHORT_STRING_MAX_LEN {
            return computed_key_by_bytes(key_bits);
        }
        let data = key_bits & crate::value::SHORT_STRING_DATA_MASK & ((1u64 << (8 * len)) - 1);
        let mut h: u64 = 0xcbf29ce484222325;
        let mut rest = data;
        let mut digits = true;
        for _ in 0..len {
            let b = rest & 0xFF;
            h = (h ^ b).wrapping_mul(0x100000001b3);
            digits &= b.wrapping_sub(b'0' as u64) < 10;
            rest >>= 8;
        }
        if digits {
            return None;
        }
        let a = crate::string::short_key_atom(len, data, h);
        if a.is_null() {
            return None;
        }
        return Some((STRING_TAG | a as u64, h));
    }
    let h = key_word_hash(key_bits)?;
    Some((key_bits, h))
}

/// [`computed_key`]'s first sight of plain heap string `s` as a key (no key
/// word yet, or a unique-owner string that never takes one): its atom and
/// hash by its bytes; null for an index-like text and for a text no atom
/// names.
#[cold]
#[inline(never)]
unsafe fn heap_key_first(s: *const crate::StringHeader) -> (*const crate::StringHeader, u64) {
    let text = std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
    if index_like(text) {
        return (std::ptr::null(), 0);
    }
    crate::string::key_first_atom(s)
}

/// [`computed_key`] for a string value by its bytes (the forms the word
/// arms above do not take).
#[cold]
#[inline(never)]
unsafe fn computed_key_by_bytes(key_bits: u64) -> Option<(u64, u64)> {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let text = crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso)?;
    if index_like(text) {
        return None;
    }
    let h = crate::object::key_bytes_hash(text.as_ptr(), text.len());
    let a = crate::string::atom_lookup(text, h)?;
    crate::string::is_property_atom(a).then_some((STRING_TAG | a as u64, h))
}

/// Is `key_bits` a string value whose text is a property key the keyed
/// probe resolves (not index-like)? Then a store may mint its atom
/// (`string::property_atom_from_value`) and ask the probe with it. Allocates
/// nothing.
///
/// # Safety
/// Any value bits.
#[cold]
#[inline(never)]
pub(crate) unsafe fn store_key_mintable(key_bits: u64) -> bool {
    let tag = key_bits & TAG_MASK;
    if tag != STRING_TAG && tag != crate::value::SHORT_STRING_TAG {
        return false;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    match crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso) {
        Some(text) => !index_like(text),
        None => false,
    }
}

/// `obj[key]` for a computed `key` (a string or symbol value): the probe
/// with the key's atom, or `TAG_HOLE` for the site's complete keyed read.
///
/// # Safety
/// `dir` as for [`perry_probe_get`]; any value bits.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_keyed(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
) -> f64 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return hole();
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return hole();
    }
    let Some((key, hash)) = computed_key(key_bits) else {
        return hole();
    };
    let v = probe_get::<true>(
        dir,
        (payload - perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
        key,
        hash,
    );
    v
}

/// What the keyed store probe ([`probe_store_keyed`]) did with
/// `obj[key] = value`.
pub(crate) enum KeyedStore {
    /// Stored: `key` is the receiver's own plain writable data property (an
    /// inline slot or an overflow entry), one barriered store.
    Stored,
    /// Nothing stored: the receiver's complete own list proves the key (this
    /// atom) absent, the receiver may grow and nothing on its prototype
    /// chain intercepts the add. The caller appends the atom to the
    /// receiver's overflow (a computed key makes no layout transition, §3.3).
    Append(*mut ObjectHeader, u64),
    /// Nothing stored: the complete store continues with this key (the key's
    /// atom: the same property, resolved once).
    Complete(u64),
    /// Nothing stored: the key has no atom (a text no atom names yet, an
    /// index-like text, or not a string or symbol at all).
    Unresolved,
}

/// `obj[key] = value` for a computed `key`: one key resolution, then the
/// store probe's plan for the receiver, for an overwrite and for an add
/// alike.
///
/// GC leaf: no allocation on the heap, no collection, no user code.
///
/// # Safety
/// Any value bits.
#[inline(always)]
pub(crate) unsafe fn probe_store_keyed(obj_bits: u64, key_bits: u64, value: f64) -> KeyedStore {
    let Some((key, hash)) = computed_key(key_bits) else {
        return KeyedStore::Unresolved;
    };
    let dir = super::super::ordinary_dir_addr();
    match super::set_plan(dir, obj_bits, key, hash, value) {
        super::SetPlan::Stored(stored) if stored != 0 => KeyedStore::Stored,
        super::SetPlan::Add(obj, r, _) => {
            if append_admitted(dir, obj, r, key, hash) {
                KeyedStore::Append(obj, key)
            } else {
                KeyedStore::Complete(key)
            }
        }
        // A decline: an overflowed receiver whose overflow does not hold the
        // key is the same add.
        super::SetPlan::Stored(_) => match overflowed_append(dir, obj_bits, key, hash) {
            Some(obj) => KeyedStore::Append(obj, key),
            None => KeyedStore::Complete(key),
        },
    }
}

/// [`KeyedStore::Append`] for a receiver that has an overflow already: the
/// overflow owns every current own key, so a key it does not hold, which the
/// layout does not name either (a deleted layout key is re-added as a
/// reference to its slot, by the complete store), is absent, and the add is
/// [`append_admitted`]'s question. `None` for every other decline of the
/// store probe.
#[inline(never)]
unsafe fn overflowed_append(
    dir: *const u8,
    obj_bits: u64,
    key: u64,
    hash: u64,
) -> Option<*mut ObjectHeader> {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return None;
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    let obj = payload as *mut ObjectHeader;
    let word = (*obj).parent_class_id;
    if word & crate::object::overflow::SHADOW_BIT == 0 {
        return None;
    }
    let r = record_of(dir, LayoutWord(word));
    if r.is_null() || (*r).object_kind() != super::super::ShapeObjectKind::Ordinary {
        return None;
    }
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null() || (*meta).overflow == 0 {
        return None;
    }
    let t = (*meta).overflow as usize as *mut crate::object::overflow::Table;
    if crate::object::overflow::find_hashed(t, key, hash).is_some()
        || own_position(r, key, hash) != Err(true)
    {
        return None;
    }
    append_admitted(dir, obj, r, key, hash).then_some(obj)
}

/// May `obj` (record `r`, whose complete own list proves `key` absent; no
/// overflow) take string atom `key` as a new own data property in its
/// overflow? The receiver facts the dynamic store asks
/// (`shape_store::try_dynamic_store`: an ordinary, extensible realm-default
/// or null-prototype bag with no descriptors and no exotic surface, a key
/// that is not `__proto__`, a private name or digit-led), read from the
/// header, the record and the meta the probe already holds, and the chain's
/// answer from the layouts ([`super::write::chain_admits_add`]). A family
/// object and a stable-tombstone receiver have their own add paths. `false`
/// decides nothing: the complete store asks again by name.
#[inline(never)]
unsafe fn append_admitted(
    dir: *const u8,
    obj: *mut ObjectHeader,
    r: *const ShapeRecord,
    key: u64,
    hash: u64,
) -> bool {
    if key & TAG_MASK != STRING_TAG || hash == super::write::PROTO_KEY_HASH {
        return false;
    }
    let first = *crate::string::string_data((key & POINTER_MASK) as *const crate::StringHeader);
    if first == b'#' || first.is_ascii_digit() {
        return false;
    }
    let gc = crate::gc::header_from_trusted_user_ptr(obj as *const u8);
    if (*gc)._reserved
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
            | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
        != 0
        || (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || crate::object::layout_desc::is_family_object(obj)
        || !matches!(
            (*r).proto_id,
            super::super::PROTO_ID_DEFAULT | super::super::PROTO_ID_NULL
        )
    {
        return false;
    }
    let id = crate::object::layout_desc::layout_id_of_word((*obj).parent_class_id);
    if id.wrapping_sub(super::super::SHAPE_ID_BASE) >= (2 << crate::codegen_abi::LAYOUT_BAND_SHIFT)
    {
        return false;
    }
    // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags
                & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
                != 0)
    {
        return false;
    }
    super::write::chain_admits_add(dir, obj, r, key, hash)
}

/// The store twin of [`perry_probe_get_keyed`]: `obj[key] = value` for a
/// computed `key` (a string or symbol value), by the store probe
/// ([`perry_probe_set`]) with the key's atom. `true` when it stored; `false`
/// for everything the complete `[[Set]]` must do (an index-like or
/// uninterned key, and every decline of the store probe).
///
/// GC leaf: no allocation on the heap, no collection, no user code.
///
/// # Safety
/// `dir` as for [`perry_probe_get`]; any value bits.
pub(crate) unsafe fn probe_set_keyed(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    value: f64,
) -> bool {
    let Some((key, hash)) = computed_key(key_bits) else {
        return false;
    };
    perry_probe_set(dir, obj_bits, key, hash, value) != 0
}
