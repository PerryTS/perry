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
#[inline]
pub(super) unsafe fn computed_key(key_bits: u64) -> Option<(u64, u64)> {
    if let Some(h) = key_word_hash(key_bits) {
        if key_bits & TAG_MASK == STRING_TAG {
            let s = (key_bits & POINTER_MASK) as *const crate::StringHeader;
            let text =
                std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
            if index_like(text) {
                return None;
            }
        }
        return Some((key_bits, h));
    }
    let tag = key_bits & TAG_MASK;
    if tag != STRING_TAG && tag != crate::value::SHORT_STRING_TAG {
        return None;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let text = crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso)?;
    if index_like(text) {
        return None;
    }
    let h = crate::object::key_bytes_hash(text.as_ptr(), text.len());
    let a = crate::string::atom_lookup(text, h)?;
    crate::string::is_property_atom(a).then_some((STRING_TAG | a as u64, h))
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
