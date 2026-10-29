//! Constructor layouts are immutable tails of the invoked body's info.
//! Binding operands name existing rooted globals/captures; no JS runs at admission.
use super::{
    layout_desc::{self, PerryLayoutDesc},
    ObjectHeader,
};
use crate::codegen_abi::LayoutWord;
use crate::{closure::ClosureHeader, gc::RuntimeHandle};

#[repr(C)]
#[derive(Clone, Copy)]
struct ConstructorLayout {
    layout: *const PerryLayoutDesc,
    operands: *const ConstructorOperand,
    count: u32,
    _reserved: u32,
}

const _: () =
    assert!(std::mem::size_of::<ConstructorLayout>() == 2 * std::mem::size_of::<usize>() + 8);
const _: () =
    assert!(std::mem::offset_of!(ConstructorLayout, count) == 2 * std::mem::size_of::<usize>());

/// Source 0 names an existing rooted global. Source 1 reads the current
/// closure capture; source 2 peeks its captured box/scope slot without TDZ checks.
/// Source 3 is an immutable compiler-emitted reader: one load of the existing
/// binding (including its current agent's TLS instance), without JavaScript or a TDZ check.
#[repr(C)]
struct ConstructorOperand {
    global: *const u64,
    capture: u32,
    kind: u16,
    slot: u16,
}

const _: () =
    assert!(std::mem::size_of::<ConstructorOperand>() == std::mem::size_of::<usize>() + 8);

unsafe fn operand_value(source: &ConstructorOperand, closure: *const ClosureHeader) -> Option<u64> {
    if source.kind == 0 {
        return source.global.as_ref().copied();
    }
    if source.kind == 3 {
        if source.global.is_null() {
            return None;
        }
        let read: unsafe extern "C" fn() -> u64 = std::mem::transmute(source.global);
        return Some(read());
    }
    if source.capture >= crate::closure::real_capture_count((*closure).capture_count) {
        return None;
    }
    let value = crate::closure::js_closure_get_capture_bits(closure, source.capture);
    if source.kind == 1 {
        return Some(value);
    }
    if source.kind != 2 {
        return None;
    }
    let address = (value & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(address)?;
    let slots = match header.obj_type {
        crate::gc::GC_TYPE_BOX => 1,
        crate::gc::GC_TYPE_SCOPE => crate::r#box::scope::scope_slot_count(header),
        _ => return None,
    };
    (usize::from(source.slot) < slots).then(|| *((address as *const u64).add(source.slot as usize)))
}

/// Read only a tail licensed by this exact immutable info. Source and name
/// retain their existing offsets; the constructor record follows both.
#[inline]
unsafe fn constructor_layout(closure: *const ClosureHeader) -> Option<&'static ConstructorLayout> {
    let info = (*closure).info.as_ref()?;
    if info.flags & crate::codegen_abi::FN_HAS_CONSTRUCTOR_LAYOUT == 0 {
        return None;
    }
    let source = crate::codegen_abi::function_source_tail_size(info);
    let name = info.flags & crate::codegen_abi::FN_HAS_NAME != 0;
    let offset = std::mem::size_of_val(info) + source + 8 * usize::from(name);
    let ptr = (*closure)
        .info
        .cast::<u8>()
        .add(offset)
        .cast::<ConstructorLayout>();
    ptr.as_ref()
}

/// A miss retains ordinary construction, including unresolved/TDZ symbol
/// operands and closures whose current keys differ from the registered layout.
/// `function` and `prototype` remain rooted across every allocation.
#[inline]
pub(crate) unsafe fn allocate(
    function: &RuntimeHandle<'_>,
    prototype: &RuntimeHandle<'_>,
) -> Option<*mut ObjectHeader> {
    let closure = (function.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ClosureHeader;
    let tail = constructor_layout(closure)?;
    allocate_layout(prototype, closure, tail)
}

/// Does `record`'s key list hold exactly `bound` at the descriptor's binding
/// symbol positions (in key order)? A record is interned by its key words, so
/// another binding of the same body names another record.
unsafe fn binds_current_symbols(
    record: super::shapes::ShapeRecordRef,
    d: &PerryLayoutDesc,
    bound: &[u64],
) -> bool {
    if bound.is_empty() {
        return true;
    }
    let keys = record.keys() as *const crate::array::ArrayHeader;
    if keys.is_null() {
        return false;
    }
    let words = crate::array::array_elements_ptr(keys);
    let mut next = 0;
    for i in 0..d.key_count as usize {
        if (*(*d.keys.add(i)).atom).kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL {
            if next >= bound.len() || *words.add(i) != bound[next] {
                return false;
            }
            next += 1;
        }
    }
    true
}

/// The image address every [`birth_tag`] is relative to.
static BIRTH_TAG_ANCHOR: u8 = 0;

/// A constructor layout descriptor's identity in 32 bits, for a prototype's
/// `ObjectMeta::function_layout_birth`: its signed distance from an anchor in
/// the same image. Descriptors are immutable image data, so the distance is
/// fixed and distinct per descriptor; one too far away (never, for an image
/// under 2 GiB) has no holder fact and always interns.
#[inline]
fn birth_tag(desc: *const PerryLayoutDesc) -> Option<u32> {
    let delta = (desc as isize).wrapping_sub(std::ptr::addr_of!(BIRTH_TAG_ANCHOR) as isize);
    i32::try_from(delta).ok().map(|delta| delta as u32)
}

unsafe fn allocate_layout(
    prototype: &RuntimeHandle<'_>,
    closure: *const ClosureHeader,
    tail: &ConstructorLayout,
) -> Option<*mut ObjectHeader> {
    let d = tail.layout.as_ref()?;
    if d.variant != crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR || tail.count > 64 {
        return None;
    }
    let prototype_bits = prototype.get_nanbox_u64();
    let p = (prototype_bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    let proto_id = crate::object::header::MetaObj::runtime_born(p as *mut _)
        .meta()
        .as_ref()?
        .proto_serial;
    if proto_id == 0 {
        return None;
    }
    let mut bound = [0u64; 64];
    for i in 0..tail.count as usize {
        let value = operand_value(&*tail.operands.add(i), closure)?;
        if crate::symbol::js_is_symbol(f64::from_bits(value)) == 0 || bound[..i].contains(&value) {
            return None;
        }
        bound[i] = value;
    }
    let bound = &bound[..tail.count as usize];
    // register_one_bound copies keys under suppression. Existing registrations
    // must match every symbol, including different factories/cross-module aliases.
    let tag = birth_tag(tail.layout);
    let _no_gc = crate::gc::GcSuppressScope::new();
    // The record the instances are born with is a fact on the holder (the
    // prototype's metadata), validated on every use as main validates its
    // descriptor cell: same layout, the record still present, it names this
    // prototype identity and serial, and its bound symbol keys are the
    // current bindings. Otherwise intern by facts and rewrite the holder.
    let meta = crate::object::header::MetaObj::runtime_born(p as *mut _).meta();
    let held = (*meta).function_layout_birth;
    let holds = tag.is_some_and(|tag| held >> 32 == u64::from(tag))
        && super::shapes::shape_record_for_word(LayoutWord(held as u32)).is_some_and(|record| {
            record.proto_id() == proto_id
                && record.prototype_word() == prototype_bits
                && binds_current_symbols(record, d, bound)
        });
    let id = if holds {
        held as u32
    } else {
        super::shapes::write_identity_word(proto_id, prototype_bits);
        let id = layout_desc::register_function_layout(tail.layout, 0, bound, proto_id);
        if id == 0
            || !binds_current_symbols(
                super::shapes::shape_record_for_word(LayoutWord(id))?,
                d,
                bound,
            )
        {
            return None;
        }
        if let Some(tag) = tag {
            // GC_STORE_AUDIT(POINTER_FREE): a scalar birth word on the rooted
            // prototype's own metadata, under suppression; no child edge.
            (*meta).function_layout_birth = (u64::from(tag) << 32) | u64::from(id);
        }
        id
    };
    // Suppression stays up through the birth: the record is carried by no one
    // until the object is stamped with its word.
    let object = layout_desc::alloc_function_layout(tail.layout, LayoutWord(id));
    drop(_no_gc);
    // As for an ordinary Object.create birth, a black newborn shades the
    // existing external prototype edge without allocating receiver metadata.
    crate::gc::runtime_shade_external_edge(prototype.get_nanbox_u64());
    Some(object)
}

#[cfg(test)]
#[path = "function_family_birth_tests.rs"]
mod tests;

/// The shape at this fixed position proves the actual symbol identity. The
/// prototype walk is needed only for a hole; present own data shadows it.
unsafe fn try_symbol_store(
    desc: *const PerryLayoutDesc,
    position: u32,
    target: f64,
    key: f64,
    value: f64,
) -> bool {
    use crate::value::{POINTER_MASK, POINTER_TAG, TAG_HOLE, TAG_MASK};
    let bits = target.to_bits();
    if bits & TAG_MASK != POINTER_TAG {
        return false;
    }
    let address = (bits & POINTER_MASK) as usize;
    let Some(gc) = crate::value::addr_class::try_read_gc_header(address) else {
        return false;
    };
    if !crate::gc::gc_type_is_object(gc.obj_type)
        || gc._reserved
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND)
            != 0
    {
        return false;
    }
    let obj = address as *mut ObjectHeader;
    let d = &*desc;
    if d.variant != crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
        || position >= d.key_count
    {
        return false;
    }
    let k = &*d.keys.add(position as usize);
    if k.attrs != crate::codegen_abi::LAYOUT_KEY_OPTIONAL
        || k.slot != position
        || k.slot >= super::object_inline_capacity(obj)
        || super::proto_validity::object_is_marked_prototype(address)
    {
        return false;
    }
    if !function_layout_admits_store(obj, d, position, key) {
        return false;
    }
    let _no_gc = crate::gc::GcSuppressScope::new();
    if *super::header::object_slot_at(obj as *mut _, u32::from(k.off)) == TAG_HOLE {
        if (position + 1..d.key_count).any(|i| {
            *super::header::object_slot_at(obj as *mut _, u32::from((*d.keys.add(i as usize)).off))
                != TAG_HOLE
        }) {
            return false;
        }
        if !symbol_chain_allows_add(target, key) {
            return false;
        }
        crate::symbol::note_symbol_key_installed((key.to_bits() & POINTER_MASK) as usize);
    }
    super::store_object_field_slot(obj, k.slot as usize, value.to_bits());
    true
}

/// Does the receiver carry exactly this descriptor's function-constructor
/// layout, with the key at `position` a plain writable data property stored
/// at slot `position`?
///
/// The shared descriptor holds no per-evaluation word (each evaluation's
/// record is interned by facts), so the fact is on the holder, as for the
/// birth itself (`allocate_layout`): the receiver's prototype's
/// `ObjectMeta::function_layout_birth` names this descriptor and holds the
/// word its instances are born with, and that holder was validated against
/// the record (prototype identity, serial, current symbol bindings) when it
/// was written. The receiver's own word equal to it, validated here on every
/// use, IS that layout: records are immutable facts, so any key add, delete,
/// attribute change, generalization, prototype change or shadowing restamps
/// the receiver with another word and fails the compare. The receiver names
/// its prototype through its own record, so the holder read is the one the
/// birth wrote for that prototype. The key at `position` is still compared
/// directly, and the entry must be the descriptor's optional plain data
/// entry: anything else takes the complete `[[Set]]`.
#[inline]
unsafe fn function_layout_admits_store(
    obj: *const ObjectHeader,
    d: &PerryLayoutDesc,
    position: u32,
    key: f64,
) -> bool {
    let word = (*obj).parent_class_id;
    if !super::shapes::is_site_matchable_layout_word(word)
        || (position < 32 && (d.rep >> (2 * position)) & 0b11 != 0)
    {
        return false;
    }
    // A1_UNPROVEN: record-first: d layout kind (kind-A/B question for S0f).
    let meta = crate::object::header::MetaObj::runtime_born(obj as *mut _).meta();
    if !meta.is_null() && (*meta).flags & super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0 {
        return false;
    }
    let Some(tag) = birth_tag(d) else {
        return false;
    };
    // The receiver is the carrier of its own word.
    let Some(record) = super::shapes::shape_record_of_receiver(word) else {
        return false;
    };
    let prototype_bits = record.prototype_word();
    if prototype_bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return false;
    }
    // Any receiver can reach here, so its prototype may be any heap kind;
    // only an ordinary object carries the holder metadata.
    let address = (prototype_bits & crate::value::POINTER_MASK) as usize;
    if crate::value::addr_class::try_read_gc_header(address)
        .is_none_or(|gc| !crate::gc::object_has_meta_word(gc.obj_type))
    {
        return false;
    }
    let prototype = address as *const ObjectHeader;
    // A1_UNPROVEN: record-first: d layout kind (kind-A/B question for S0f).
    let Some(holder) = crate::object::header::MetaObj::runtime_born(prototype as *mut _)
        .meta()
        .as_ref()
    else {
        return false;
    };
    if holder.function_layout_birth != (u64::from(tag) << 32) | u64::from(word) {
        return false;
    }
    let keys = record.keys() as *const crate::array::ArrayHeader;
    if keys.is_null()
        || *crate::array::array_elements_ptr(keys).add(position as usize) != key.to_bits()
    {
        return false;
    }
    let entry = super::key_attrs::keys_entry(keys, position);
    entry == super::key_attrs::ENTRY_OPTIONAL
        && super::key_attrs::entry_is_plain_writable_data(entry)
}

/// Only ordinary physical prototype layouts are admitted, and every live
/// descriptor is checked anew. No accessor or user code is invoked here.
unsafe fn symbol_chain_allows_add(receiver: f64, key: f64) -> bool {
    use crate::value::{POINTER_MASK, POINTER_TAG, TAG_MASK, TAG_NULL};
    let mut prototype = super::js_object_get_prototype_of(receiver);
    for _ in 0..64 {
        let bits = prototype.to_bits();
        if bits == TAG_NULL {
            return true;
        }
        if bits & TAG_MASK != POINTER_TAG {
            return false;
        }
        let address = (bits & POINTER_MASK) as usize;
        let Some(gc) = crate::value::addr_class::try_read_gc_header(address) else {
            return false;
        };
        if !crate::gc::gc_type_is_object(gc.obj_type) {
            return false;
        }
        let obj = address as *const ObjectHeader;
        if (*obj).class_id != 0
            || (!crate::object::header::object_meta_if_present(obj as *mut _, gc.obj_type)
                .is_null()
                && (*crate::object::header::object_meta_if_present(obj as *mut _, gc.obj_type))
                    .flags
                    & super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    != 0)
        {
            return false;
        }
        if let Some(entry) =
            super::shaped_symbols::entry(address, (key.to_bits() & POINTER_MASK) as usize)
        {
            return super::key_attrs::entry_is_plain_writable_data(entry);
        }
        prototype = super::js_object_get_prototype_of(prototype);
    }
    false
}

/// Checked fixed-position symbol store, preserving full borrowed-call and
/// descriptor semantics on a miss. Operands are evaluated exactly once.
#[no_mangle]
pub unsafe extern "C-unwind" fn perry_function_family_symbol_set(
    desc: *const PerryLayoutDesc,
    position: u32,
    target: f64,
    key: f64,
    value: f64,
    strict: i32,
) -> f64 {
    if try_symbol_store(desc, position, target, key, value) {
        return value;
    }
    crate::proxy::js_put_value_set(target, key, value, target, strict)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_FUNCTION_FAMILY_SYMBOL_SET: unsafe extern "C-unwind" fn(
    *const PerryLayoutDesc,
    u32,
    f64,
    f64,
    f64,
    i32,
) -> f64 = perry_function_family_symbol_set;
