//! Constructor layouts are immutable tails of the invoked body's info.
//! Binding operands name existing rooted globals/captures; no JS runs at admission.
use super::{
    layout_desc::{self, PerryLayoutDesc},
    ObjectHeader,
};
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

unsafe fn allocate_layout(
    prototype: &RuntimeHandle<'_>,
    closure: *const ClosureHeader,
    tail: &ConstructorLayout,
) -> Option<*mut ObjectHeader> {
    let d = tail.layout.as_ref()?;
    if d.variant != crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR || tail.count > 64 {
        return None;
    }
    let existing = layout_desc::registered_layout_id(layout_desc::load_layout_id(tail.layout));
    let p = (prototype.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ObjectHeader;
    let meta = (*p).meta.as_ref()?;
    let proto_id = meta.proto_serial;
    if proto_id == 0 {
        return None;
    }
    if let Some(record) = existing.and_then(super::shapes::shape_record_by_id) {
        if record.proto_id() != proto_id || record.prototype_word() != prototype.get_nanbox_u64() {
            return None;
        }
        if tail.count == 0 {
            let object = layout_desc::alloc_function_layout(tail.layout);
            crate::gc::runtime_shade_external_edge(prototype.get_nanbox_u64());
            return Some(object);
        }
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
    let _no_gc = crate::gc::GcSuppressScope::new();
    super::shapes::write_identity_word(proto_id, prototype.get_nanbox_u64());
    let id =
        layout_desc::register_function_layout(tail.layout, existing.unwrap_or(0), bound, proto_id);
    if id == 0 {
        return None;
    }
    let shape = super::shapes::shape_descriptor_by_id(id)?;
    let keys = shape.keys_view();
    let mut next = 0;
    for i in 0..d.key_count {
        if (*(*d.keys.add(i as usize)).atom).kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL {
            if keys.get(i).bits() != bound[next] {
                return None;
            }
            next += 1;
        }
    }
    drop(_no_gc);
    let object = layout_desc::alloc_function_layout(tail.layout);
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
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
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
    let id = layout_desc::load_layout_id(desc);
    if d.variant != crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
        || (*obj).parent_class_id != id
        || position >= d.key_count
    {
        return false;
    }
    let Some(record) = super::shapes::shape_record_by_id(id) else {
        return false;
    };
    let keys = record.keys() as *const crate::array::ArrayHeader;
    if keys.is_null()
        || *crate::array::array_elements_ptr(keys).add(position as usize) != key.to_bits()
    {
        return false;
    }
    let k = &*d.keys.add(position as usize);
    if k.attrs != crate::codegen_abi::LAYOUT_KEY_OPTIONAL
        || k.slot >= super::object_inline_capacity(obj)
        || super::proto_validity::object_is_marked_prototype(address)
    {
        return false;
    }
    let slots = obj
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>();
    let _no_gc = crate::gc::GcSuppressScope::new();
    if *slots.add(k.slot as usize) == TAG_HOLE {
        if (position + 1..d.key_count)
            .any(|i| *slots.add((*d.keys.add(i as usize)).slot as usize) != TAG_HOLE)
        {
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
        if gc.obj_type != crate::gc::GC_TYPE_OBJECT {
            return false;
        }
        let obj = address as *const ObjectHeader;
        if (*obj).class_id != 0
            || (!(*obj).meta.is_null()
                && (*(*obj).meta).flags & super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
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
