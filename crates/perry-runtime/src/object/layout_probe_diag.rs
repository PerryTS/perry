use super::*;

/// `PERRY_PROBE_DIAG=1`: why the probe declined `key` on `obj` (one stderr
/// line per slow read; diagnostics only, never on a probe path).
pub(crate) fn diag_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_PROBE_DIAG").as_deref() == Ok("1"))
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
                let r = record_of(super::super::ordinary_dir_addr(), word);
                if r.is_null() {
                    format!("noflat{:x}", word >> 24)
                } else if !(*r).present() {
                    String::from("absentrec")
                } else {
                    let p = probe_word(r);
                    let kind = (*r).object_kind();
                    let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
                    let shared = !keys.is_null() && super::super::keys_prefix_is_immutable(keys);
                    let proto = match prototype_of(super::super::ordinary_dir_addr(), o, r) {
                        Next::Object(_) => 1usize,
                        Next::End => 2,
                        Next::Unknown => 0,
                    };
                    format!(
                        "kind={kind:?} posbound={} count={} gen0={} holes={} shared={shared} atoms={} decline={} shadow={} sum={:x} protoid={:x} class={} next={}",
                        (*r).position_bound_raw(),
                        (*r).logical_key_count,
                        super::super::complete_layout_generation((*r).semantic_generation),
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
pub(super) unsafe fn diag_walk(obj: *const ObjectHeader, key_bits: u64, hash: u64) -> String {
    let dir = super::super::ordinary_dir_addr();
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
                    super::super::complete_layout_generation((*rn).semantic_generation),
                    (*rn).hole_count,
                    (*rn).proto_id == super::super::PROTO_ID_PER_OBJECT,
                    (*rn).element_kind,
                    !keys.is_null() && super::super::keys_prefix_is_immutable(keys),
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

/// Probe verify mode (`PERRY_PROBE_VERIFY=1`), a debugging instrument: every
/// answer the probe gives (a value or a proven absence) is held back, the
/// probe declines, and the site's slow entry checks the held answer against
/// the complete `[[Get]]` (`Reflect.get`) before it runs. The first
/// disagreements print the key, both answers and the receiver's chain.
pub(crate) mod verify {
    use super::super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

    static ON: AtomicBool = AtomicBool::new(false);
    static CHECKED: AtomicU64 = AtomicU64::new(0);
    static BAD: AtomicU64 = AtomicU64::new(0);
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
        let dir = super::super::super::ordinary_dir_addr();
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
                super::super::super::complete_layout_generation(rr.semantic_generation),
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
