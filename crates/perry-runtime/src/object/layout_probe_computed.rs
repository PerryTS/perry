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
            let meta = (*obj).meta;
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
                let fields =
                    (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
                let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
                let mut hole = false;
                if keys[pos as usize..].iter().any(|k| {
                    let present = *fields.add(k.slot as usize) != TAG_HOLE;
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
            let r = record_of(dir, word);
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

/// The canonical word and hash of a computed property key, as a site key
/// would carry them: a property atom or a symbol as it is; a short or plain
/// string as its text's property atom when one exists. `None` for anything
/// else (a number, an object whose ToPropertyKey runs user code, a text no
/// atom names).
#[inline]
unsafe fn computed_key(key_bits: u64) -> Option<(u64, u64)> {
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

/// `PERRY_PROBE_DIAG=1`: why the probe declined `key` on `obj` (one stderr
/// line per slow read; diagnostics only, never on a probe path).
pub(crate) fn diag_enabled() -> bool {
    use std::sync::OnceLock;
    per_test_global! {
        static ON: OnceLock<bool> = OnceLock::new();
    }
    *ON.get_or_init(|| std::env::var("PERRY_PROBE_DIAG").as_deref() == Ok("1"))
}

/// See [`diag_enabled`]: one `PROBE_ACC <key>` stderr line per read whose
/// slow entry called the getter the layout walk found (no complete read).
///
/// # Safety
/// `key` is a live string.
pub(crate) unsafe fn diag_accessor(key: *const crate::StringHeader) {
    let name = String::from_utf8_lossy(std::slice::from_raw_parts(
        crate::string::string_data(key),
        (*key).byte_len as usize,
    ))
    .into_owned();
    eprintln!("PROBE_ACC {name}");
}

/// See [`diag_enabled`].
///
/// # Safety
/// `obj_handle` is the slow entry's receiver operand; `key` a live string.
pub(crate) unsafe fn diag_explain(obj_handle: i64, key: *const crate::StringHeader) {
    let name = if key.is_null() {
        String::from("?")
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(
            crate::string::string_data(key),
            (*key).byte_len as usize,
        ))
        .into_owned()
    };
    let obj = obj_handle as usize;
    let why = if obj < perry_abi::RECEIVER_HANDLE_FLOOR {
        String::from("handle")
    } else {
        match crate::value::addr_class::try_read_gc_header(obj) {
            Some(h) if h.obj_type != crate::gc::GC_TYPE_OBJECT => format!("gctype{}", h.obj_type),
            None => String::from("nogc"),
            Some(_) => {
                let o = obj as *const ObjectHeader;
                let word = (*o).parent_class_id;
                let r = record_of(super::ordinary_dir_addr(), word);
                if r.is_null() {
                    format!("noflat{:x}", word >> 24)
                } else if !(*r).present() {
                    String::from("absentrec")
                } else {
                    let p = probe_word(r);
                    let kind = (*r).object_kind();
                    let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
                    let shared = !keys.is_null() && super::keys_prefix_is_immutable(keys);
                    let proto = match prototype_of(super::ordinary_dir_addr(), o, r) {
                        Next::Object(_) => 1usize,
                        Next::End => 2,
                        Next::Unknown => 0,
                    };
                    format!(
                        "kind={kind:?} posbound={} count={} gen0={} holes={} shared={shared} atoms={} decline={} shadow={} sum={:x} protoid={:x} class={} next={}",
                        (*r).position_bound_raw(),
                        (*r).logical_key_count,
                        super::complete_layout_generation((*r).semantic_generation),
                        (*r).hole_count,
                        p & PROBE_ATOMS != 0,
                        p & PROBE_DECLINE != 0,
                        word & crate::object::overflow::SHADOW_BIT != 0,
                        (*r).summary(),
                        (*r).proto_id >> 56,
                        (*o).class_id,
                        proto,
                    )
                }
            }
        }
    };
    let walk = if obj >= perry_abi::RECEIVER_HANDLE_FLOOR && !key.is_null() {
        let bytes =
            std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
        let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
        diag_walk(obj as *const ObjectHeader, STRING_TAG | key as u64, hash)
    } else {
        String::new()
    };
    eprintln!("PROBE_DIAG {name} {why} walk={walk}");
}

/// Where [`inherited_get`]'s walk stops for `key_bits` on `obj`.
unsafe fn diag_walk(obj: *const ObjectHeader, key_bits: u64, hash: u64) -> String {
    let dir = super::ordinary_dir_addr();
    let mut cur = obj;
    let mut rc = record_of(dir, (*obj).parent_class_id) as *const ShapeRecord;
    if rc.is_null() {
        return "norecord".into();
    }
    let acc = |r: *const ShapeRecord, p: u32| {
        let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() && crate::object::key_attrs::key_is_accessor_at(keys, p) {
            ":acc"
        } else {
            ""
        }
    };
    match own_position(rc as *mut ShapeRecord, key_bits, hash) {
        Ok(p) => return format!("own@{p}{}", acc(rc, p)),
        Err(false) => return "own-unproven".into(),
        Err(true) => {}
    }
    for hop in 0..MAX_HOPS {
        if !walks_from(cur, rc) {
            return format!("hop{hop}:nowalk({:?})", (*rc).object_kind());
        }
        let next = match prototype_of(dir, cur, rc) {
            Next::Object(p) => p,
            Next::End => return format!("hop{hop}:end"),
            Next::Unknown => {
                return format!(
                    "hop{hop}:unknown(protoid={:x},cell={},class={})",
                    (*rc).proto_id,
                    (*rc).prototype_word() != 0,
                    (*cur).class_id,
                )
            }
        };
        let rn = record_of(dir, (*next).parent_class_id);
        if rn.is_null() {
            return format!("hop{hop}:proto-norecord");
        }
        match own_position(rn, key_bits, hash) {
            Ok(p) => {
                let v = value_at(next, rn, p);
                return format!(
                    "hop{hop}:found@{p}:{}{}",
                    if v.to_bits() == TAG_HOLE {
                        "hole"
                    } else {
                        "value"
                    },
                    acc(rn, p)
                );
            }
            Err(false) => {
                let pw = (*rn).probe;
                let keys = (*rn).keys as usize as *const crate::array::ArrayHeader;
                return format!(
                    "hop{hop}:proto-unproven(kind={:?},atoms={},decline={},objproto={},gen0={},holes={},perobj={},elem={},shared={},count={})",
                    (*rn).object_kind(),
                    pw & PROBE_ATOMS != 0,
                    pw & PROBE_DECLINE != 0,
                    next as usize == crate::array::object_prototype_addr_if_resolved(),
                    super::complete_layout_generation((*rn).semantic_generation),
                    (*rn).hole_count,
                    (*rn).proto_id == super::PROTO_ID_PER_OBJECT,
                    (*rn).element_kind,
                    !keys.is_null() && super::keys_prefix_is_immutable(keys),
                    (*rn).logical_key_count,
                );
            }
            Err(true) => {}
        }
        if next as usize == crate::array::object_prototype_addr_if_resolved() {
            return format!("hop{hop}:objproto-miss");
        }
        cur = next;
        rc = rn;
    }
    "maxhops".into()
}

/// This agent's ordinary shape directory (tests).
#[cfg(test)]
pub(crate) fn test_dir() -> *const u8 {
    super::ordinary_dir_addr()
}

/// Key position `pos` of `obj`'s record's list (tests).
#[cfg(test)]
pub(crate) unsafe fn test_list_word(obj: *const ObjectHeader, pos: usize) -> u64 {
    let r = record_of(super::ordinary_dir_addr(), (*obj).parent_class_id);
    assert!(!r.is_null(), "the object has an ordinary record");
    *(crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64).add(pos)
}

/// Probe verify mode (`PERRY_PROBE_VERIFY=1`), a debugging instrument: every
/// answer the probe gives (a value or a proven absence) is held back, the
/// probe declines, and the site's slow entry checks the held answer against
/// the complete `[[Get]]` (`Reflect.get`) before it runs. The first
/// disagreements print the key, both answers and the receiver's chain.
pub(crate) mod verify {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

    per_test_global! {
        static ON: AtomicBool = AtomicBool::new(false);
    }
    static CHECKED: AtomicU64 = AtomicU64::new(0);
    per_test_global! {
        static BAD: AtomicU64 = AtomicU64::new(0);
    }
    /// (receiver address, the site's key bits, the probe's answer).
    #[thread_local]
    static HELD: Cell<(usize, u64, u64)> = Cell::new((0, 0, 0));

    extern "C" fn summary() {
        eprintln!(
            "PROBE_VERIFY checked={} disagreements={}",
            CHECKED.load(Relaxed),
            BAD.load(Relaxed)
        );
    }

    pub(crate) fn init() {
        if std::env::var("PERRY_PROBE_VERIFY").as_deref() == Ok("1") && !ON.swap(true, Relaxed) {
            // SAFETY: registering a plain extern "C" function.
            unsafe { libc::atexit(summary) };
        }
    }

    #[inline(always)]
    pub(crate) fn on() -> bool {
        ON.load(Relaxed)
    }

    #[inline(always)]
    pub(crate) fn answer(obj: usize, key_bits: u64, v: f64) -> f64 {
        if ON.load(Relaxed) && v.to_bits() != TAG_HOLE {
            HELD.set((obj, key_bits, v.to_bits()));
            return hole();
        }
        v
    }

    /// The slow entry for `key_bits` on the object at `obj`: check a held
    /// answer for exactly this read.
    #[cold]
    pub(crate) unsafe fn check(obj: usize, key_bits: u64) {
        if !ON.load(Relaxed) {
            return;
        }
        let (o, k, a) = HELD.replace((0, 0, 0));
        // A computed key is held as its canonical atom (`computed_key`).
        if o != obj || o == 0 || (k != key_bits && computed_key(key_bits).map(|c| c.0) != Some(k)) {
            return;
        }
        let boxed = f64::from_bits(POINTER_TAG | obj as u64);
        let reference =
            crate::proxy::js_reflect_get(boxed, f64::from_bits(key_bits), boxed).to_bits();
        let n = CHECKED.fetch_add(1, Relaxed) + 1;
        if reference == a {
            return;
        }
        let bad = BAD.fetch_add(1, Relaxed) + 1;
        if bad <= 40 {
            eprintln!(
                "PROBE_VERIFY_BAD #{bad} (of {n}) key={} probe={a:#x} get={reference:#x} chain={}",
                key_text(key_bits),
                chain(obj as *const ObjectHeader, key_bits)
            );
        }
    }

    /// The slow entry's accessor for `key_bits` on the object at `obj`
    /// (found by the layout walk, [`probe_accessor_word`]): the getter
    /// `[[Get]]` would call, by the complete descriptor walk
    /// (`__lookupGetter__`), must be that accessor's.
    #[cold]
    pub(crate) unsafe fn accessor(
        obj: usize,
        key_bits: u64,
        acc: &crate::object::accessor_pair::Accessor,
    ) {
        if !ON.load(Relaxed) {
            return;
        }
        let boxed = f64::from_bits(POINTER_TAG | obj as u64);
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let namespace_name = ((*(obj as *const ObjectHeader)).class_id
            == crate::object::namespace_create::MODULE_NAMESPACE_CLASS_ID)
            .then(|| {
                crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso)
            })
            .flatten();
        let reference = match namespace_name {
            // A namespace's live binding is an accessor inside and a data
            // property to reflection (10.4.6.5): the reference is the
            // complete own-descriptor walk beneath reflection.
            Some(name) => {
                crate::object::descriptors::namespace_binding_getter(obj as *mut ObjectHeader, name)
            }
            None => {
                crate::object::js_object_lookup_getter(boxed, f64::from_bits(key_bits)).to_bits()
            }
        };
        let n = CHECKED.fetch_add(1, Relaxed) + 1;
        let agrees = if acc.get != 0 {
            reference == acc.get
        } else if acc.raw_get != 0 || acc.static_get != 0 {
            // A class getter's function object is made on demand.
            crate::closure::is_closure_bits(reference)
        } else {
            reference == crate::value::TAG_UNDEFINED
        };
        if agrees {
            return;
        }
        let bad = BAD.fetch_add(1, Relaxed) + 1;
        if bad <= 40 {
            eprintln!(
                "PROBE_VERIFY_BAD #{bad} (of {n}) key={} accessor get={:#x} raw={:#x} lookup={reference:#x} chain={}",
                key_text(key_bits),
                acc.get,
                acc.raw_get,
                chain(obj as *const ObjectHeader, key_bits)
            );
        }
    }

    /// A compiled family slot access (codegen emits `perry_family_slot_verify`
    /// at each one when `PERRY_PROBE_VERIFY=1` while compiling): the
    /// receiver's record must hold `key_bits` at storage slot `slot`. The
    /// site's slot is the compiler's family layout (`ProgramLayouts`); the
    /// record is what the runtime built from the registered descriptor and
    /// what the probe and `[[Get]]` both read, so a descriptor or record that
    /// disagrees with the code that births and reads its objects shows here,
    /// where the probe-against-`[[Get]]` check above sees one record twice.
    #[cold]
    pub(crate) unsafe fn slot(obj_bits: u64, key_bits: u64, slot: u32) {
        if !ON.load(Relaxed) {
            return;
        }
        let obj = (obj_bits & POINTER_MASK) as *const ObjectHeader;
        let word = (*obj).parent_class_id;
        let r = record_of(super::super::ordinary_dir_addr(), word);
        let found = if r.is_null() {
            None
        } else {
            let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
            let list = crate::array::array_elements_ptr(keys) as *const u64;
            (0..(*r).logical_key_count)
                .find(|&i| {
                    let w = *list.add(i as usize);
                    w == key_bits || text_entry_matches(w, key_bits)
                })
                .map(|pos| crate::object::key_attrs::key_storage_slot(keys, pos))
        };
        let n = CHECKED.fetch_add(1, Relaxed) + 1;
        if found == Some(slot) {
            return;
        }
        let bad = BAD.fetch_add(1, Relaxed) + 1;
        if bad <= 40 {
            eprintln!(
                "PROBE_VERIFY_BAD #{bad} (of {n}) family slot key={} compiled={slot} record={} word={word:#x}",
                key_text(key_bits),
                found.map_or("absent".into(), |s| s.to_string())
            );
        }
    }

    unsafe fn key_text(bits: u64) -> String {
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(t) =
            crate::string::js_string_key_bytes(crate::JSValue::from_bits(bits), &mut sso)
        {
            return String::from_utf8_lossy(t).into_owned();
        }
        if crate::symbol::js_is_symbol(f64::from_bits(bits)) != 0 {
            let sh = (bits & POINTER_MASK) as *const crate::symbol::SymbolHeader;
            let d = (*sh).description;
            return if d.is_null() {
                "Symbol()".into()
            } else {
                format!(
                    "Symbol({})",
                    String::from_utf8_lossy(std::slice::from_raw_parts(
                        crate::string::string_data(d),
                        (*d).byte_len as usize
                    ))
                )
            };
        }
        format!("{bits:#x}")
    }

    unsafe fn chain(obj: *const ObjectHeader, key_bits: u64) -> String {
        let hash = key_word_hash(key_bits).unwrap_or(0);
        let dir = super::super::ordinary_dir_addr();
        let mut out = String::new();
        let mut cur = obj;
        for _ in 0..MAX_HOPS {
            let word = (*cur).parent_class_id;
            let r = record_of(dir, word);
            let gc = crate::value::addr_class::try_read_gc_header(cur as usize);
            let meta = (*cur).meta;
            let mproto = if meta.is_null() { 0 } else { (*meta).prototype };
            let mflags = if meta.is_null() {
                0
            } else {
                (*meta).flags as u64
            };
            out.push_str(&format!(
                "[obj={:#x} gctype={:?} class={} word={word:#x} objproto={} meta.proto={mproto:#x} meta.flags={mflags:#x} ",
                cur as usize,
                gc.map(|h| h.obj_type),
                (*cur).class_id,
                object_prototype(dir) == Some(cur),
            ));
            if r.is_null() {
                out.push_str("norecord]");
                break;
            }
            let rr = &*r;
            let keys = rr.keys as usize as *const crate::array::ArrayHeader;
            let mut names = Vec::new();
            if !keys.is_null() {
                let words = crate::array::array_elements_ptr(keys) as *const u64;
                let scan =
                    (0..rr.logical_key_count as usize).position(|i| *words.add(i) == key_bits);
                let texts = (0..rr.logical_key_count as usize)
                    .filter(|&i| key_text(*words.add(i)) == key_text(key_bits))
                    .map(|i| (i, *words.add(i), key_word_hash(*words.add(i))))
                    .collect::<Vec<_>>();
                out.push_str(&format!(
                    "scan={scan:?} own={:?} hash={hash:#x} same_text={texts:x?} ",
                    own_position(r, key_bits, hash)
                ));
                for i in 0..(rr.logical_key_count as usize).min(24) {
                    names.push(key_text(*words.add(i)));
                }
            }
            out.push_str(&format!(
                "kind={:?} count={} gen0={} holes={} protoid={:#x} probe={:#x} sum={:#x} posbound={} keys={names:?}] ",
                rr.object_kind(),
                rr.logical_key_count,
                super::super::complete_layout_generation(rr.semantic_generation),
                rr.hole_count,
                rr.proto_id,
                rr.probe & PROBE_FLAGS,
                rr.summary(),
                rr.position_bound_raw(),
            ));
            match prototype_of(dir, cur, r) {
                Next::Object(p) => cur = p,
                Next::End => {
                    out.push_str("END");
                    break;
                }
                Next::Unknown => {
                    out.push_str("UNKNOWN");
                    break;
                }
            }
        }
        out
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_PROBE_GET_VALUE: unsafe extern "C" fn(*const u8, u64, u64, u64) -> f64 =
    perry_probe_get_value;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_PROBE_GET: unsafe extern "C" fn(*const u8, i64, u64, u64) -> f64 =
    perry_probe_get;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_PROBE_GET_BINDING: unsafe extern "C" fn(*const u8, i64, u64, u64) -> f64 =
    perry_probe_get_binding;

#[cfg(test)]
#[path = "builtin_layout_probe_tests.rs"]
mod builtin_layout_probe_tests;
