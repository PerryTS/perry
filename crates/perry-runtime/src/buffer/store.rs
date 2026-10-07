//! The shared byte cell, its single traced link and ordinary property bag.
use super::BufferHeader;
use crate::object::ObjectHeader;
use crate::value::JSValue;

pub(crate) const ARRAY_BUFFER_KEY: &str = "#<perry:array-buffer>";
pub(crate) const VIEW_OWNER_KEY: &str = "#<perry:view-owner>";
pub(crate) const PIN_OVERFLOW_KEY: &str = "#<perry:pin-overflow>";
pub(crate) const PROTOTYPE_KEY: &str = "#<perry:prototype>";

const _: () = assert!(std::mem::size_of::<BufferHeader>() == crate::codegen_abi::BYTES_STORE);
const _: () = assert!(std::mem::offset_of!(BufferHeader, link) == crate::codegen_abi::BYTES_LINK);

#[inline(always)]
pub(crate) unsafe fn header(addr: usize) -> *mut crate::gc::GcHeader {
    crate::gc::header_from_trusted_user_ptr(addr as *const u8).cast_mut()
}

#[inline(always)]
pub(crate) fn is_view(addr: usize) -> bool {
    unsafe { crate::gc::is_byte_view_type((*header(addr)).obj_type) }
}

#[inline]
pub(crate) unsafe fn bag(addr: usize) -> *mut ObjectHeader {
    let link = (*(addr as *const BufferHeader)).link;
    if link != 0 && (*header(link)).obj_type == crate::gc::GC_TYPE_OBJECT {
        link as *mut ObjectHeader
    } else {
        std::ptr::null_mut()
    }
}

pub(crate) unsafe fn own_slot(obj: *const ObjectHeader, key: &[u8]) -> Option<u32> {
    if obj.is_null() {
        return None;
    }
    let keys = crate::object::object_keys(obj);
    crate::object::keys_find_slot_by_bytes_resolved(keys.arr(), keys.count(), key)
}

pub(crate) unsafe fn bag_get(addr: usize, key: &str) -> Option<f64> {
    let obj = bag(addr);
    let slot = own_slot(obj, key.as_bytes())?;
    let value = crate::object::object_field_at_with_live(
        obj,
        slot,
        crate::object::object_live_slot_count(obj),
    );
    (value.bits() != crate::value::TAG_HOLE).then(|| f64::from_bits(value.bits()))
}

/// Both the cell and its bytes are nonmoving. Bag allocation never moves them.
pub(crate) unsafe fn bag_ensure(addr: usize) -> *mut ObjectHeader {
    let existing = bag(addr);
    if !existing.is_null() {
        return existing;
    }
    let _suppress = crate::gc::GcSuppressScope::new();
    #[cfg(test)]
    if super::bytes::b4_sabotage("attach_moves_bytes") {
        super::header::externalize_on_attach_for_test(addr);
    }
    let cell = addr as *mut BufferHeader;
    let owner = (*cell).link;
    let obj = crate::object::js_object_alloc_null_proto(0, 0);
    if is_view(addr) {
        object_define(
            obj,
            VIEW_OWNER_KEY,
            crate::value::js_nanbox_pointer(owner as i64),
            true,
        );
    }
    // GC_STORE_AUDIT(BARRIERED): the sole raw-pointer child edge of a byte cell.
    (*cell).link = obj as usize;
    crate::gc::runtime_write_barrier_slot(
        addr,
        std::ptr::addr_of!((*cell).link) as usize,
        obj as u64,
    );
    obj
}

pub(crate) unsafe fn object_define(obj: *mut ObjectHeader, key: &str, value: f64, hidden: bool) {
    let name = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::object_ops::define_property_force_store_value(obj, name, value);
    if hidden {
        #[cfg(test)]
        let writable = super::bytes::b4_sabotage("private_key_descriptor");
        #[cfg(not(test))]
        let writable = false;
        crate::object::descriptor_state::note_descriptor_target_edits(
            obj as usize,
            &[crate::object::key_attrs::AttrsEdit::Data(
                key.as_bytes(),
                // Engine-owned keys remain ordinary shaped properties, but
                // public assignment/redefinition cannot replace an owner
                // edge or pin count. Trusted updates use the force-store
                // funnel above, including after freeze/preventExtensions.
                crate::object::PropertyAttrs::new(writable, false, writable).bits,
            )],
        );
    }
}

pub(crate) unsafe fn bag_set(addr: usize, key: &str, value: f64, hidden: bool) {
    let _suppress = crate::gc::GcSuppressScope::new();
    let obj = bag_ensure(addr);
    object_define(obj, key, value, hidden);
}

#[inline]
pub(crate) unsafe fn owner(addr: usize) -> usize {
    if !is_view(addr) {
        return addr;
    }
    let link = (*(addr as *const BufferHeader)).link;
    if (*header(link)).obj_type == crate::gc::GC_TYPE_OBJECT {
        let value = bag_get(addr, VIEW_OWNER_KEY).expect("view bag must retain its owner");
        JSValue::from_bits(value.to_bits()).as_pointer::<u8>() as usize
    } else {
        link
    }
}

#[inline(always)]
pub(crate) unsafe fn owner_data(addr: usize) -> *mut u8 {
    let bytes = (addr as *mut u8).add(crate::codegen_abi::BYTES_STORE);
    if (*header(addr))._reserved & crate::codegen_abi::BYTES_OUT_OF_LINE != 0 {
        *(bytes as *const *mut u8)
    } else {
        bytes
    }
}

#[inline(always)]
pub(crate) unsafe fn element_size(addr: usize) -> usize {
    1usize << crate::codegen_abi::BYTES_ELEMENT_SHIFT[((*header(addr)).obj_type & 0x1f) as usize]
}

pub(crate) unsafe fn owner_byte_length(addr: usize) -> usize {
    (*(addr as *const BufferHeader)).length as usize * element_size(addr)
}

#[inline(always)]
pub(crate) unsafe fn set_length(addr: usize, len: u32) {
    (*(addr as *mut BufferHeader)).length = len;
}

pub(crate) unsafe fn clear_owner_extent(addr: usize) {
    let cell = addr as *mut BufferHeader;
    (*cell).length = 0;
    (*cell).capacity = 0;
}

pub(crate) unsafe fn length(addr: usize) -> usize {
    let cell = &*(addr as *const BufferHeader);
    if !is_view(addr) {
        return cell.length as usize;
    }
    let owning = owner(addr);
    if (*header(owning))._reserved & crate::codegen_abi::BYTES_DETACHED != 0 {
        return 0;
    }
    let available = owner_byte_length(owning);
    let offset = cell.capacity as usize;
    if offset > available {
        return 0;
    }
    let size = element_size(addr);
    if (*header(addr))._reserved & crate::codegen_abi::BYTES_LENGTH_TRACKING != 0 {
        (available - offset) / size
    } else if cell.length as usize * size <= available - offset {
        cell.length as usize
    } else {
        0
    }
}

pub(crate) unsafe fn out_of_bounds(addr: usize) -> bool {
    if !is_view(addr) {
        return false;
    }
    let cell = &*(addr as *const BufferHeader);
    let owning = owner(addr);
    let available = owner_byte_length(owning);
    let offset = cell.capacity as usize;
    offset > available
        || ((*header(addr))._reserved & crate::codegen_abi::BYTES_LENGTH_TRACKING == 0
            && cell.length as usize * element_size(addr) > available - offset)
}

pub(crate) unsafe fn data(addr: usize) -> *mut u8 {
    if is_view(addr) {
        owner_data(owner(addr)).add((*(addr as *const BufferHeader)).capacity as usize)
    } else {
        owner_data(addr)
    }
}

pub(crate) fn new_view(
    brand: u8,
    source: usize,
    offset: u32,
    length: u32,
    tracking: bool,
) -> *mut BufferHeader {
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let owning = owner(source);
        let offset = offset
            .checked_add(if is_view(source) {
                (*(source as *const BufferHeader)).capacity
            } else {
                0
            })
            .expect("view offset overflow");
        let _ = crate::object::shape_rule3::checked_plus_four_word(
            offset,
            b"Invalid typed array length",
        );
        let cell = crate::arena::arena_alloc_gc_old(
            crate::codegen_abi::BYTES_STORE,
            8,
            (brand & !0x20) | 0x20,
        ) as *mut BufferHeader;
        (*cell).length = length;
        (*cell).capacity = offset;
        (*cell).link = owning;
        (*header(cell as usize)).gc_flags |= crate::gc::GC_FLAG_TENURED;
        if tracking && (brand & 0x1f == 14 || super::is_resizable_buffer(owning)) {
            (*header(cell as usize))._reserved |= crate::codegen_abi::BYTES_LENGTH_TRACKING;
        }
        crate::gc::runtime_write_barrier_slot(
            cell as usize,
            std::ptr::addr_of!((*cell).link) as usize,
            owning as u64,
        );
        cell
    }
}
