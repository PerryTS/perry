//! The shared byte cell, its single traced link and ordinary property bag.

/// Opaque shared header for every byte-family owner and view.
/// Field access belongs to store; emitted access uses perry-abi constants.
// The byte store starts at offset 16 on every target (BYTES_STORE), and
// typed-array elements may require 8-byte alignment. Round the ILP32 header
// up from 12 bytes as well, preserving the native layout and link offset.
#[repr(C, align(8))]
pub struct BufferHeader {
    /// Length in bytes
    length: u32,
    /// Capacity (allocated space)
    capacity: u32,
    /// Owner or ordinary shaped property bag; the cell's only traced edge.
    link: usize,
}

pub(crate) mod layout;

use crate::object::ObjectHeader;
use crate::value::JSValue;

pub(crate) const ARRAY_BUFFER_KEY: &str = "#<perry:array-buffer>";
pub(crate) const VIEW_OWNER_KEY: &str = "#<perry:view-owner>";
pub(crate) const PIN_OVERFLOW_KEY: &str = "#<perry:pin-overflow>";
pub(crate) const PROTOTYPE_KEY: &str = "#<perry:prototype>";

const _: () = assert!(std::mem::size_of::<BufferHeader>() == crate::codegen_abi::BYTES_STORE);
const _: () = assert!(std::mem::offset_of!(BufferHeader, link) == crate::codegen_abi::BYTES_LINK);
const _: () =
    assert!(std::mem::size_of::<ObjectHeader>() == crate::codegen_abi::BYTES_VIEW_BAG_OWNER);
const _: () = assert!(crate::gc::GC_TYPE_OBJECT == crate::codegen_abi::GC_TYPE_OBJECT);

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

/// Whether `bag` (non-null) holds a live own value under `key`.
pub(crate) unsafe fn bag_holds(bag: *const ObjectHeader, key: &[u8]) -> bool {
    own_slot(bag, key).is_some_and(|slot| {
        crate::object::object_field_at_with_live(
            bag,
            slot,
            crate::object::object_live_slot_count(bag),
        )
        .bits()
            != crate::value::TAG_HOLE
    })
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
    let obj = if is_view(addr) {
        // The owner is the bag's first key, born in inline slot 0 with its
        // final attributes, so `BYTES_VIEW_BAG_OWNER` reaches it in one load.
        #[cfg(test)]
        if super::bytes::b4_sabotage("view_bag_owner_not_first") {
            // Planted fault: another key is born first, so the owner is not
            // at `BYTES_VIEW_BAG_OWNER`.
            let obj = crate::object::js_object_alloc_null_proto(0, 0);
            object_define(obj, "sabotage", 0.0, false);
            object_define(
                obj,
                VIEW_OWNER_KEY,
                crate::value::js_nanbox_pointer(owner as i64),
                true,
            );
            return attach_bag(cell, obj);
        }
        crate::object::alloc::object_alloc_null_proto_with_key_attrs(
            &[(
                VIEW_OWNER_KEY,
                crate::value::js_nanbox_pointer(owner as i64),
            )],
            &[crate::object::key_attrs::attr_bits_to_entry(hidden_attrs())],
        )
    } else {
        crate::object::js_object_alloc_null_proto(0, 0)
    };
    attach_bag(cell, obj)
}

unsafe fn attach_bag(cell: *mut BufferHeader, obj: *mut ObjectHeader) -> *mut ObjectHeader {
    let addr = cell as usize;
    // GC_STORE_AUDIT(BARRIERED): the sole raw-pointer child edge of a byte cell.
    (*cell).link = obj as usize;
    crate::gc::runtime_write_barrier_slot(
        addr,
        std::ptr::addr_of!((*cell).link) as usize,
        obj as u64,
    );
    obj
}

/// The attributes of an engine-owned bag key: neither writable, enumerable
/// nor configurable, at bag birth and at a later define alike.
fn hidden_attrs() -> u8 {
    #[cfg(test)]
    let writable = super::bytes::b4_sabotage("private_key_descriptor");
    #[cfg(not(test))]
    let writable = false;
    crate::object::PropertyAttrs::new(writable, false, writable).bits
}

pub(crate) unsafe fn object_define(obj: *mut ObjectHeader, key: &str, value: f64, hidden: bool) {
    let name = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::object_ops::define_property_force_store_value(obj, name, value);
    if hidden {
        crate::object::descriptor_state::note_descriptor_target_edits(
            obj as usize,
            &[crate::object::key_attrs::AttrsEdit::Data(
                key.as_bytes(),
                // Engine-owned keys remain ordinary shaped properties, but
                // public assignment/redefinition cannot replace an owner
                // edge or pin count. Trusted updates use the force-store
                // funnel above, including after freeze/preventExtensions.
                hidden_attrs(),
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
        view_bag_owner(link)
    } else {
        link
    }
}

/// The owner a view's bag holds at its fixed first inline slot.
#[inline(always)]
pub(crate) unsafe fn view_bag_owner(bag: usize) -> usize {
    let value = *((bag + crate::codegen_abi::BYTES_VIEW_BAG_OWNER) as *const u64);
    JSValue::from_bits(value).as_pointer::<u8>() as usize
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

/// Initialization and provenance at the one store entry. PoolCopy/PoolUnsafe
/// identify precisely the copying Buffer.from, allocUnsafe and concat paths.
pub(crate) enum Init<'a> {
    Zero,
    Uninit,
    Copy(&'a [u8]),
    PoolCopy,
    PoolUnsafe,
    AdoptVec(Vec<u8>),
    AdoptBacking(super::backing::Backing),
    Foreign(*mut u8),
    Shared,
}

pub(crate) fn store_alloc(brand: u8, len: u32, init: Init<'_>) -> *mut BufferHeader {
    use super::backing::Backing;
    use crate::gc::ByteStorePlacement;
    super::bytes::assert_allocation_allowed();
    let size = 1usize << crate::codegen_abi::BYTES_ELEMENT_SHIFT[(brand & 0x1f) as usize];
    let byte_len = (len as usize)
        .checked_mul(size)
        .filter(|&n| n <= crate::object::shape_rule3::MAX_PLUS_FOUR_WORD as usize)
        .unwrap_or_else(|| crate::typedarray::throw_range_error(b"Array buffer allocation failed"));
    let placement = crate::gc::byte_store_placement(brand, &init, byte_len);
    #[cfg(test)]
    let placement = if super::bytes::native_copy_fixture() {
        ByteStorePlacement::Native
    } else if super::bytes::sabotage("large_inline") {
        ByteStorePlacement::Inline
    } else {
        placement
    };
    let capacity = byte_len as u32;
    let ptr = match init {
        Init::Shared => {
            assert_eq!(brand, crate::gc::GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER);
            return crate::shared_sab::alloc_shared_sab_impl(capacity);
        }
        Init::Foreign(data) => super::header::alloc_foreign(brand, data, len),
        Init::AdoptBacking(backing) => super::header::alloc_backing(brand, backing, len),
        init => {
            let ptr = match placement {
                ByteStorePlacement::PoolView { size } => super::pool::alloc_view(size, len),
                ByteStorePlacement::Native => {
                    let backing = match init {
                        Init::Zero => Backing::zeroed(capacity),
                        Init::Copy(body) => {
                            assert_eq!(body.len(), byte_len);
                            unsafe { Backing::copy(body.as_ptr(), capacity) }
                        }
                        Init::AdoptVec(body) => {
                            assert_eq!(body.len(), byte_len);
                            #[cfg(test)]
                            let body = if super::bytes::sabotage("adopt_copy") {
                                body.clone()
                            } else {
                                body
                            };
                            Backing::from_vec(body)
                        }
                        _ => Backing::uninit(capacity),
                    };
                    return super::header::alloc_backing(brand, backing, len);
                }
                ByteStorePlacement::Inline => super::header::alloc_inline(brand, capacity, len),
            };
            unsafe {
                match init {
                    Init::Zero => std::ptr::write_bytes(data(ptr as usize), 0, byte_len),
                    Init::Copy(body) => {
                        assert_eq!(body.len(), byte_len);
                        std::ptr::copy_nonoverlapping(body.as_ptr(), data(ptr as usize), byte_len);
                    }
                    Init::AdoptVec(body) => {
                        assert_eq!(body.len(), byte_len);
                        std::ptr::copy_nonoverlapping(body.as_ptr(), data(ptr as usize), byte_len);
                    }
                    Init::Uninit | Init::PoolCopy | Init::PoolUnsafe => (),
                    _ => unreachable!(),
                }
            }
            ptr
        }
    };
    ptr
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[inline(always)]
pub(crate) unsafe fn capacity(addr: usize) -> u32 {
    (*(addr as *const BufferHeader)).capacity
}
#[inline(always)]
pub(crate) unsafe fn initialize_shared_block(cell: *mut BufferHeader, size: u32) {
    std::ptr::write(
        cell,
        BufferHeader {
            length: size,
            capacity: size,
            link: 0,
        },
    );
}
#[cfg(test)]
pub(crate) unsafe fn raw_link(addr: usize) -> usize {
    (*(addr as *const BufferHeader)).link
}
#[cfg(test)]
pub(crate) unsafe fn set_test_link(addr: usize, link: usize) {
    (*(addr as *mut BufferHeader)).link = link;
}

#[cfg(test)]
pub(crate) fn alloc_test(brand: u8, capacity: u32) -> *mut BufferHeader {
    let cell = store_alloc(brand, capacity, Init::Uninit);
    unsafe {
        set_length(cell as usize, 0);
    }
    cell
}

/// The stored element count, before view bounds/length-tracking resolution.
#[inline(always)]
pub(crate) unsafe fn raw_length(addr: usize) -> u32 {
    (*(addr as *const BufferHeader)).length
}
/// The collector alone rewrites this edge; callers never expose a derived byte pointer.
#[inline(always)]
pub(crate) unsafe fn gc_link_slot(addr: usize) -> Option<*mut usize> {
    let cell = addr as *mut BufferHeader;
    ((*cell).link != 0).then(|| std::ptr::addr_of_mut!((*cell).link))
}

#[cfg(test)]
pub(crate) unsafe fn set_test_capacity(addr: usize, cap: u32) {
    (*(addr as *mut BufferHeader)).capacity = cap;
}

#[cfg(test)]
pub(crate) unsafe fn sabotage_inline_copy(value: f64, input: &[u8]) {
    // Restore the old +8 copy so the witness detects pointer-word corruption.
    let cell = JSValue::from_bits(value.to_bits())
        .as_pointer::<u8>()
        .cast_mut();
    std::ptr::copy_nonoverlapping(input.as_ptr(), cell.add(8), input.len());
}
