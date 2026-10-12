//! Instance facts are owned by a stable collecting native cell. The host's
//! opaque context word names that cell, while every JS wrapper owns an edge.
use crate::native_handle::NativeHandleHeader;
use crate::value::JSValue;
use std::ffi::c_void;
const TYPE_ID: u64 = 0x1179_8efb_4213_c8ac;
static VTABLE: crate::native_payload::PayloadVTable = crate::native_payload::PayloadVTable {
    drop: drop_state,
    stream: None,
};
unsafe extern "C" fn drop_state(resource: *mut c_void, _hint: *mut c_void) {
    drop(Box::from_raw(resource.cast::<MemorySpan>()));
}
pub(crate) fn new(imports: f64) -> f64 {
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let bag = crate::array::js_array_alloc_with_length(3);
        crate::array::js_array_set_f64(bag, 0, imports);
        let cell = crate::native_handle::native_handle_new_rust_payload(
            Box::into_raw(Box::new(MemorySpan {
                instance: 0,
                data: 0,
                len: 0,
                rebound: false,
            }))
            .cast(),
            TYPE_ID,
            &VTABLE,
            "Perry.Wasm.InstanceState",
            false,
        );
        crate::gc::runtime_store_gc_jsvalue_slot(
            cell as usize,
            std::ptr::addr_of_mut!((*cell).owner) as usize,
            crate::value::js_nanbox_pointer(bag as i64).to_bits(),
        );
        crate::native_handle::native_handle_set_external_bytes(
            cell,
            std::mem::size_of::<MemorySpan>(),
        );
        crate::value::js_nanbox_pointer(cell as i64)
    }
}
pub(crate) fn token(context: f64) -> u64 {
    context.to_bits() & crate::value::POINTER_MASK
}
unsafe fn cell(context: f64) -> Option<*mut NativeHandleHeader> {
    let value = JSValue::from_bits(context.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let cell = value.as_pointer::<NativeHandleHeader>() as *mut NativeHandleHeader;
    let header = crate::value::addr_class::try_read_tracked_gc_header(cell as usize)?;
    if header.as_ref().obj_type != crate::gc::GC_TYPE_NATIVE_HANDLE {
        return None;
    }
    (!crate::native_handle::native_handle_rust_payload_ptr(cell, TYPE_ID).is_null()).then_some(cell)
}
#[derive(Clone, Copy)]
pub(crate) struct MemorySpan {
    pub instance: usize,
    pub data: usize,
    pub len: usize,
    pub rebound: bool,
}
fn load(context: f64, slot: u32) -> f64 {
    unsafe {
        let Some(cell) = cell(context) else {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        };
        let bag = JSValue::from_bits((*cell).owner);
        if !bag.is_pointer() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        crate::array::js_array_get_f64(bag.as_pointer::<crate::array::ArrayHeader>(), slot)
    }
}
pub(crate) fn imports(token: u64) -> f64 {
    load(crate::value::js_nanbox_pointer(token as i64), 0)
}
pub(crate) fn buffer(context: f64) -> f64 {
    load(context, 1)
}
pub(crate) fn memory(context: f64) -> f64 {
    load(context, 2)
}
pub(crate) fn set_memory(context: f64, memory: f64) {
    store(context, 2, memory);
}
fn store(context: f64, slot: u32, value: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let cell = cell(context).expect("live Wasm instance context");
        let bag = JSValue::from_bits((*cell).owner).as_pointer::<crate::array::ArrayHeader>();
        crate::array::js_array_set_f64(bag as *mut crate::array::ArrayHeader, slot, value);
    }
}
pub(crate) fn set_buffer(context: f64, buffer: f64) {
    store(context, 1, buffer);
}
pub(crate) fn memory_span(context: f64) -> Option<MemorySpan> {
    unsafe {
        Some(
            *crate::native_handle::native_handle_rust_payload_ptr(cell(context)?, TYPE_ID)
                .cast::<MemorySpan>(),
        )
    }
}
pub(crate) fn set_memory_span(context: f64, span: MemorySpan) {
    unsafe {
        let cell = cell(context).expect("live Wasm instance context");
        *crate::native_handle::native_handle_rust_payload_ptr(cell, TYPE_ID).cast::<MemorySpan>() =
            span;
    }
}
pub(crate) fn attach(owner: f64, context: f64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let owner = JSValue::from_bits(owner.to_bits());
    if !owner.is_pointer() {
        return;
    }
    unsafe {
        crate::object::owned_state_bag::store(
            owner.as_pointer::<u8>() as usize,
            crate::object::owned_state_bag::WASM_INSTANCE,
            context,
        );
    }
}
pub(crate) fn from_owner(owner: f64) -> f64 {
    let owner = JSValue::from_bits(owner.to_bits());
    unsafe {
        if owner.is_pointer() {
            crate::object::owned_state_bag::load(
                owner.as_pointer::<u8>() as usize,
                crate::object::owned_state_bag::WASM_INSTANCE,
            )
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
}
#[cfg(test)]
pub(crate) unsafe fn clear_imports(context: f64) {
    if let Some(cell) = cell(context) {
        let bag = JSValue::from_bits((*cell).owner).as_pointer::<crate::array::ArrayHeader>();
        crate::array::js_array_set_f64(
            bag as *mut crate::array::ArrayHeader,
            0,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }
}
