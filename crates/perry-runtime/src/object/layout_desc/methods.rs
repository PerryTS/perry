//! Immutable family method indices. The receiver's carried layout identifies
//! this table; each entry names the complete static holder chain and its slot.
//! A changed holder word, or a shadowed receiver, takes ordinary dispatch.
use super::*;
use crate::codegen_abi::LayoutWord;

#[repr(C)]
pub struct PerryFamilyMethods {
    pub mask: u32,
    pub _pad: u32,
    pub entries: *const PerryFamilyMethod,
}
#[repr(C)]
pub struct PerryFamilyMethod {
    pub atom: *const StaticAtom,
    pub guards: *const PerryMethodHolder,
    pub depth: u32,
    pub slot: u32,
}
#[repr(C)]
pub struct PerryMethodHolder {
    pub class_id: u32,
    pub word: LayoutWord,
}
unsafe impl Sync for PerryFamilyMethods {}
unsafe impl Sync for PerryFamilyMethod {}
unsafe impl Sync for PerryMethodHolder {}
const _: () = assert!(
    std::mem::size_of::<PerryFamilyMethods>()
        == if cfg!(target_pointer_width = "32") {
            12
        } else {
            16
        }
);
const _: () = assert!(
    std::mem::size_of::<PerryFamilyMethod>()
        == if cfg!(target_pointer_width = "32") {
            16
        } else {
            24
        }
);
const _: () = assert!(std::mem::size_of::<PerryMethodHolder>() == 8);

/// The carried descriptor band (including possible class method tables).
/// This immutable word range rejects runtime and shadowed layouts cheaply.
#[inline(always)]
pub(crate) fn may_have_methods(word: u32) -> bool {
    super::word_in_layout_index(word)
}

/// A GC leaf: a layout-indexed method lookup, with no allocation, TLS lookup,
/// user code, learned state or name/holder walk. Zero means ordinary dispatch.
/// The returned function takes explicit `this`, has no rest/arguments bundling,
/// and needs no more argument registers than the caller supplies.
///
/// # Safety
/// `dir` is this agent's ordinary directory; `recv` a heap receiver biased by
/// `RECEIVER_HANDLE_FLOOR`; `key` is a live string atom, `hash` its byte hash.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_family_method(
    dir: *const u8,
    recv: i64,
    key: u64,
    hash: u64,
    argc: u32,
) -> i64 {
    let obj = (recv as usize + perry_abi::RECEIVER_HANDLE_FLOOR) as *const ObjectHeader;
    // The range test also rejects shadowed words, before touching a record
    // or a GC header. Most ordinary method receivers carry a runtime shape.
    if !may_have_methods((*obj).parent_class_id) {
        return 0;
    }
    let h = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if !crate::gc_type_is_object!((*h).obj_type)
        || (*h).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || word_is_shadowed((*obj).parent_class_id)
    {
        return 0;
    }
    let Some(d) = family_desc_of(obj) else {
        return 0;
    };
    let Some(table) = d.methods.as_ref() else {
        return 0;
    };
    if key & crate::value::TAG_MASK != crate::value::STRING_TAG {
        return 0;
    }
    let k = (key & crate::value::POINTER_MASK) as *const StringHeader;
    let bytes = std::slice::from_raw_parts(crate::string::string_data(k), (*k).byte_len as usize);
    let mut at = hash as u32 & table.mask;
    let entry = loop {
        let e = &*table.entries.add(at as usize);
        if e.atom.is_null() {
            return 0;
        }
        if (*e.atom).hash == hash && ((*e.atom).string == k || atom_bytes(e.atom) == Some(bytes)) {
            break e;
        }
        at = (at + 1) & table.mask;
    };
    let word_dir = *(dir.add(crate::codegen_abi::SHAPE_DIR_FLAT_BASE + 24) as *const *const u8);
    if word_dir.is_null() {
        return 0;
    }
    let mut holder = std::ptr::null::<ObjectHeader>();
    for guard in std::slice::from_raw_parts(entry.guards, entry.depth as usize) {
        let pid = super::super::shapes::class_identity_proto_id(guard.class_id);
        let Some(cell) = super::super::shapes::identity_word_slot_in(word_dir, pid) else {
            return 0;
        };
        let bits = *cell;
        if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
            return 0;
        }
        holder = (bits & crate::value::POINTER_MASK) as *const ObjectHeader;
        if (*holder).parent_class_id != guard.word.0 {
            return 0;
        }
    }
    if holder.is_null() {
        return 0;
    }
    let value = *((holder as *const u8)
        .add(std::mem::size_of::<ObjectHeader>() + entry.slot as usize * 8)
        as *const u64);
    // ConstFn is a shape fact. Its immutable function info specifies the ABI;
    // the closure value remains the callee environment, including lexical home.
    let closure = (value & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    let Some(info) = (*closure).info.as_ref() else {
        return 0;
    };
    if info.flags & crate::codegen_abi::FN_COMPILED_BODY == 0
        || info.flags & crate::closure::FN_REST_MASK != 0
        || u32::from(info.params) > argc
    {
        return 0;
    }
    if super::super::layout_probe::verify::answer(obj as usize, key, f64::from_bits(value))
        .to_bits()
        == crate::value::TAG_HOLE
    {
        return 0;
    }
    closure as i64
}

/// The by-id tower entrance shares the same immutable family answer. The
/// compiled caller already owns the arguments; nothing can collect before
/// the body receives them. A miss preserves the full existing dispatcher.
pub(crate) unsafe fn call_by_id(
    recv: f64,
    method_id: i64,
    args: *const f64,
    argc: usize,
) -> Option<f64> {
    let biased = recv
        .to_bits()
        .wrapping_sub(crate::value::POINTER_TAG | perry_abi::RECEIVER_HANDLE_FLOOR as u64);
    if biased >= (1u64 << 48) - perry_abi::RECEIVER_HANDLE_FLOOR as u64 {
        return None;
    }
    let obj = (biased as usize + perry_abi::RECEIVER_HANDLE_FLOOR) as *const ObjectHeader;
    if !may_have_methods((*obj).parent_class_id) {
        return None;
    }
    let d = family_desc_of(obj)?;
    if d.methods.is_null() {
        return None;
    }
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)?;
    let bytes = std::slice::from_raw_parts(name.ptr, name.len);
    let hash = if name.static_id != 0 {
        name.hash
    } else {
        super::super::key_bytes_hash(bytes.as_ptr(), bytes.len())
    };
    let key = crate::string::atom_lookup(bytes, hash)?;
    let key_bits = crate::value::STRING_TAG | key as u64;
    let callee = perry_probe_family_method(
        super::super::shapes::ordinary_dir_addr(),
        biased as i64,
        key_bits,
        hash,
        u32::try_from(argc).ok()?,
    );
    if callee == 0 {
        super::super::layout_probe::verify::check(
            (recv.to_bits() & crate::value::POINTER_MASK) as usize,
            key_bits,
        );
        return None;
    }
    let closure = callee as *const crate::closure::ClosureHeader;
    Some(
        super::super::native_call_method::compiled_target::enter_compiled_body(
            closure,
            &*(*closure).info,
            crate::closure::JsThis::from_f64(recv),
            args,
            argc,
        ),
    )
}
