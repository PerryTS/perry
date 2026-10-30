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
    let reference = crate::proxy::js_reflect_get(boxed, f64::from_bits(key_bits), boxed).to_bits();
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
        .then(|| crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut sso))
        .flatten();
    let reference = match namespace_name {
        // A namespace's live binding is an accessor inside and a data
        // property to reflection (10.4.6.5): the reference is the
        // complete own-descriptor walk beneath reflection.
        Some(name) => {
            crate::object::descriptors::namespace_binding_getter(obj as *mut ObjectHeader, name)
        }
        None => crate::object::js_object_lookup_getter(boxed, f64::from_bits(key_bits)).to_bits(),
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
    let r = record_of(super::super::ordinary_dir_addr(), LayoutWord(word));
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
    if let Some(t) = crate::string::js_string_key_bytes(crate::JSValue::from_bits(bits), &mut sso) {
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
        let r = record_of(dir, LayoutWord(word));
        let gc = crate::value::addr_class::try_read_gc_header(cur as usize);
        // A1_UNPROVEN: record-first: probe receiver before its record (kind-A/B question for S0f).
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(cur as *mut _));
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
            let scan = (0..rr.logical_key_count as usize).position(|i| *words.add(i) == key_bits);
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
