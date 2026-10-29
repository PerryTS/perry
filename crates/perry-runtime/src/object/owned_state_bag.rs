//! Private, object-owned facts share an ordinary GC array. Slots are private
//! runtime state, never public own properties or an owner-address registry.
use super::ObjectHeader;
use crate::array::ArrayHeader;

pub(crate) const ASYNC_RESOURCE: u32 = 4;
pub(crate) const WASM_INSTANCE: u32 = 5;
pub(crate) const WASI_CONTEXT: u32 = 6;
pub(crate) const COMPONENT_CONTEXT: u32 = 7;
pub(crate) const COMPONENT_IMPORTS: u32 = 8;
pub(crate) const COMPONENT_RESOURCE_OWNER: u32 = 9;
pub(crate) const COMPONENT_RESOURCE_TOKEN: u32 = 10;
pub(crate) const COMPONENT_RESOURCE_STATE: u32 = 11;
pub(crate) const COMPONENT_EXPORTS: u32 = 12;
pub(crate) const COMPONENT_GUEST: u32 = 13;
pub(crate) const COMPONENT_GUEST_RESOURCES: u32 = 14;
pub(crate) const COMPONENT_GUEST_NEXT_RESOURCE: u32 = 15;
pub(crate) const COMPONENT_GUEST_RESOURCE: u32 = 16;
pub(crate) const COMPONENT_HOST_RESOURCES: u32 = 17;
pub(crate) const COMPONENT_HOST_NEXT_RESOURCE: u32 = 18;
pub(crate) const COMPONENT_HOST_RESOURCE: u32 = 19;
pub(crate) const COMPONENT_FINALIZATION: u32 = 20;
pub(crate) const COMPONENT_RESOURCE_PARENT: u32 = 21;
pub(crate) const COMPONENT_ASYNC_IMPORTS: u32 = 22;
pub(crate) const COMPONENT_RESOURCE_BORROW: u32 = 23;
pub(crate) const COMPONENT_ASYNC_RESULT_TYPE: u32 = 24;
pub(crate) const COMPONENT_DRIVER_OWNER: u32 = 25;
pub(crate) const COMPONENT_ASYNC_VALUE: u32 = 26;
pub(crate) const COMPONENT_ASYNC_WRITER: u32 = 27;

pub(crate) unsafe fn bag(owner: usize, create: bool, last_slot: u32) -> *mut ArrayHeader {
    let _publication = crate::gc::GcSuppressScope::new();
    let _edit = if create {
        super::descriptor_state::HolderEdit::new(owner)
    } else {
        None
    };
    let holder: *mut ObjectHeader = super::descriptor_state::descriptor_holder(owner);
    if holder.is_null() {
        return std::ptr::null_mut();
    }
    let meta = if create {
        // A1_UNPROVEN: descriptor_holder(owner) may return the user object itself (kind-A/B question for S0b-S0e).
        super::object_meta_ensure(
            crate::meta_obj_runtime_born!(holder),
            super::MetaReason::OwnedState,
        )
    } else {
        // A1_UNPROVEN: record-first: holder may be the user object (kind-A/B question for S0f).
        crate::meta_read!(crate::meta_obj_runtime_born!(holder as *mut _))
    };
    if meta.is_null() {
        return std::ptr::null_mut();
    }
    let mut bag = (*meta).owned_state as *mut ArrayHeader;
    if create && (bag.is_null() || (*bag).length <= last_slot) {
        let new = crate::array::js_array_alloc_with_length(last_slot + 1);
        if !bag.is_null() {
            for i in 0..(*bag).length {
                crate::array::js_array_set(new, i, crate::array::js_array_get(bag, i));
            }
        }
        crate::gc::runtime_store_gc_heap_word_slot(
            meta as usize,
            std::ptr::addr_of_mut!((*meta).owned_state) as usize,
            new as u64,
        );
        bag = new;
    }
    bag
}

pub(crate) unsafe fn load(owner: usize, slot: u32) -> f64 {
    let bag = bag(owner, false, slot);
    if bag.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        crate::array::js_array_get_f64(bag, slot)
    }
}

pub(crate) unsafe fn store(owner: usize, slot: u32, value: f64) {
    let _publication = crate::gc::GcSuppressScope::new();
    let bag = bag(owner, true, slot);
    assert!(
        !bag.is_null(),
        "private state requires an owning property holder"
    );
    crate::array::js_array_set_f64(bag, slot, value);
}
