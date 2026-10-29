use super::*;

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
                let r = record_of(super::super::ordinary_dir_addr(), LayoutWord(word));
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
    let mut rc = record_of(dir, LayoutWord((*obj).parent_class_id)) as *const ShapeRecord;
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
        let rn = record_of(dir, LayoutWord((*next).parent_class_id));
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
                    (*rn).element_kind(),
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
