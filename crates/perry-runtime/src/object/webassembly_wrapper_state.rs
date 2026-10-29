//! The wrapper owns its trusted host handle through ObjectMeta.native_state.
//! The wasmi host retains its process-lifetime engine objects as before; this
//! binding drops only its Rust state. No owner-address registry or GC hook.
use crate::native_handle::NativeHandleHeader;
use std::ffi::c_void;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Module,
    Memory,
    Table,
    Global,
}
impl Kind {
    pub(crate) fn from_bytes(kind: &[u8]) -> Option<Self> {
        match kind {
            b"memory" => Some(Self::Memory),
            b"table" => Some(Self::Table),
            b"global" => Some(Self::Global),
            _ => None,
        }
    }
}
struct WrapperState {
    kind: Kind,
    handle: usize,
}
const TYPE_ID: u64 = 0x1179_8efb_4213_c8ae;
static VTABLE: crate::native_payload::PayloadVTable = crate::native_payload::PayloadVTable {
    drop: drop_state,
    stream: None,
};
unsafe extern "C" fn drop_state(resource: *mut c_void, _: *mut c_void) {
    drop(Box::from_raw(resource.cast::<WrapperState>()));
}
pub(crate) fn attach(wrapper: usize, kind: Kind, handle: usize) {
    if wrapper == 0 || handle == 0 {
        return;
    }
    let _publication = crate::gc::GcSuppressScope::new();
    unsafe {
        let wrapper_type = crate::value::addr_class::try_read_tracked_gc_header_of_type(
            wrapper,
            crate::gc::GC_TYPE_OBJECT,
        )
        .expect("a Wasm binding must have a genuine managed wrapper")
        .as_ref()
        .obj_type;
        let meta = crate::object::object_meta_ensure(
            crate::object::MetaObj::from_type(wrapper as *mut crate::ObjectHeader, wrapper_type),
            crate::object::MetaReason::WasmWrapper,
        );
        let cell = crate::native_handle::native_handle_new_rust_payload(
            Box::into_raw(Box::new(WrapperState { kind, handle })).cast(),
            TYPE_ID,
            &VTABLE,
            "Perry.Wasm.WrapperState",
            false,
        );
        crate::gc::runtime_store_gc_jsvalue_slot(
            meta as usize,
            std::ptr::addr_of_mut!((*meta).native_state) as usize,
            crate::value::js_nanbox_pointer(cell as i64).to_bits(),
        );
        crate::native_handle::native_handle_set_external_bytes(
            cell,
            std::mem::size_of::<WrapperState>(),
        );
    }
}
pub(crate) fn handle(wrapper: usize, kind: Kind) -> Option<usize> {
    unsafe {
        let header = crate::value::addr_class::try_read_tracked_gc_header_of_type(
            wrapper,
            crate::gc::GC_TYPE_OBJECT,
        )?;
        if header.as_ref().gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
            return None;
        }
        // A1_UNPROVEN: record-first: wrapper tested by tracked type, no t (kind-A/B question for S0f).
        let meta = crate::object::header::MetaObj::runtime_born(
            wrapper as *const crate::ObjectHeader as *mut _,
        )
        .meta();
        if meta.is_null() {
            return None;
        }
        let value = crate::JSValue::from_bits((*meta).native_state);
        if !value.is_pointer() {
            return None;
        }
        let cell = value.as_pointer::<NativeHandleHeader>() as *mut NativeHandleHeader;
        crate::value::addr_class::try_read_tracked_gc_header_of_type(
            cell as usize,
            crate::gc::GC_TYPE_NATIVE_HANDLE,
        )?;
        let state = crate::native_handle::native_handle_rust_payload_ptr(cell, TYPE_ID)
            .cast::<WrapperState>()
            .as_ref()?;
        (state.kind == kind).then_some(state.handle)
    }
}
