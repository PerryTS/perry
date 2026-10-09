//! Owner links and reopen for asynchronous binding families, using the same
//! typed vtable and cell as the stream/payload ABI.
use super::*;
use crate::native_payload as np;

/// Reopen a closed typed cell. Caller retains resource on refusal.
/// # Safety
/// Same resource/descriptor contract as js_perry_payload_attach.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_reopen(
    value: f64,
    family: *const PerryPayloadFamily,
    resource: *mut c_void,
    bytes: usize,
) -> i32 {
    let Some(family) = checked_family(family) else {
        return -1;
    };
    let Some(cell) = family_cell(value, family) else {
        return -1;
    };
    if resource.is_null() {
        return -1;
    }
    match np::attach_external_cell(
        value,
        cell,
        family_type_id(family),
        resource,
        &*family.vtable,
        bytes,
    ) {
        Ok(()) => 0,
        Err(np::AttachMiss::Foreign) => -1,
        Err(np::AttachMiss::Open) => 5,
        Err(np::AttachMiss::Closing) => 6,
        Err(np::AttachMiss::Finalized) => 7,
    }
}

/// OPEN=1, CLOSING=2, CLOSED=3; foreign=-1, finalized/wrong-thread=-2.
/// # Safety
/// family is a static descriptor.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_lifecycle(
    value: f64,
    family: *const PerryPayloadFamily,
) -> i32 {
    let Some(family) = checked_family(family) else {
        return -1;
    };
    let Some(cell) = family_cell(value, family) else {
        return -1;
    };
    if (*cell).finalized != 0
        || (*cell).creator_thread_id != crate::native_handle::current_thread_id()
    {
        return -2;
    }
    if (*cell).flags & np::CLOSING != 0 {
        2
    } else if (*cell).resource_ptr.is_null() {
        3
    } else {
        1
    }
}

/// Stable owner link, or zero for a foreign/finalized/wrong-thread/N cell.
/// # Safety
/// family is a static descriptor.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_owner_link(
    value: f64,
    family: *const PerryPayloadFamily,
) -> usize {
    let Some(family) = checked_family(family) else {
        return 0;
    };
    if family.links_owner == 0 {
        return 0;
    }
    let Some(cell) = family_cell(value, family) else {
        return 0;
    };
    if np::link_event_owner(np::OwnerLink(cell as usize)).is_none() {
        return 0;
    }
    assert!(
        (cell as u64) < np::LINK_ADDRESS_LIMIT && cell as usize & 7 == 0,
        "a payload cell must be 8-aligned below 2^56 to travel in a link token"
    );
    cell as usize
}

/// Retain a live owner-thread link until its terminal completion.
/// # Safety
/// link names a live cell on this thread.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_ref(link: usize) {
    np::link_ref(np::OwnerLink(link));
}
/// Release one outstanding completion's reference.
/// # Safety
/// link names a live cell with a matching ref on this thread.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_unref(link: usize) {
    np::link_unref(np::OwnerLink(link));
}
/// Fetch an event owner even after close; never allocates or throws.
/// # Safety
/// link is live on this thread and out is writable.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_event_owner(link: usize, out: *mut f64) -> i32 {
    if link == 0 || out.is_null() {
        return 0;
    }
    match np::link_event_owner(np::OwnerLink(link)) {
        Some(owner) => {
            // GC_STORE_AUDIT(STACK): caller's writable out-param. This helper
            // cannot allocate; the caller roots the owner before any safepoint.
            out.write(owner);
            1
        }
        None => 0,
    }
}
/// The traced JS-state object; no JS value belongs in the native payload.
/// # Safety
/// family is a static descriptor.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_js_state(
    value: f64,
    family: *const PerryPayloadFamily,
    create: i32,
) -> f64 {
    let Some(family) = checked_family(family) else {
        return bytes_undefined();
    };
    if family_cell(value, family).is_none() {
        return bytes_undefined();
    }
    // family_cell brands source subclasses by the attached cell. JS state is
    // owned by the actual object, whose class id can differ from the family.
    let obj = np::any_object(value).unwrap();
    np::js_state_for_class(value, (*obj).class_id, create != 0)
}

#[cfg(feature = "keepalive-anchors")]
mod keepalive {
    use super::*;
    #[used(compiler)]
    static REOPEN: unsafe extern "C" fn(f64, *const PerryPayloadFamily, *mut c_void, usize) -> i32 =
        js_perry_payload_reopen;
    #[used(compiler)]
    static LIFECYCLE: unsafe extern "C" fn(f64, *const PerryPayloadFamily) -> i32 =
        js_perry_payload_lifecycle;
    #[used(compiler)]
    static OWNER_LINK: unsafe extern "C" fn(f64, *const PerryPayloadFamily) -> usize =
        js_perry_payload_owner_link;
    #[used(compiler)]
    static LINK_REF: unsafe extern "C" fn(usize) = js_perry_payload_link_ref;
    #[used(compiler)]
    static LINK_UNREF: unsafe extern "C" fn(usize) = js_perry_payload_link_unref;
    #[used(compiler)]
    static EVENT_OWNER: unsafe extern "C" fn(usize, *mut f64) -> i32 =
        js_perry_payload_link_event_owner;
    #[used(compiler)]
    static JS_STATE: unsafe extern "C" fn(f64, *const PerryPayloadFamily, i32) -> f64 =
        js_perry_payload_js_state;
    #[used(compiler)]
    static LINK_PTR: unsafe extern "C" fn(
        usize,
        *const PerryPayloadFamily,
        *mut i32,
    ) -> *mut c_void = js_perry_payload_link_ptr;
    #[used(compiler)]
    static LINK_LIFECYCLE: unsafe extern "C" fn(usize, *const PerryPayloadFamily) -> i32 =
        js_perry_payload_link_lifecycle;
    #[used(compiler)]
    static LINK_CLOSE: unsafe extern "C" fn(usize, *const PerryPayloadFamily) -> i32 =
        js_perry_payload_link_close;
    #[used(compiler)]
    static LINK_REOPEN: unsafe extern "C" fn(
        usize,
        *const PerryPayloadFamily,
        *mut c_void,
        usize,
    ) -> i32 = js_perry_payload_link_reopen;
    #[used(compiler)]
    static LINK_JS_STATE: unsafe extern "C" fn(usize, *const PerryPayloadFamily, i32) -> f64 =
        js_perry_payload_link_js_state;
    #[used(compiler)]
    static LINK_BYTES: unsafe extern "C" fn(usize, *const PerryPayloadFamily, usize) =
        js_perry_payload_link_external_bytes;
    #[used(compiler)]
    static RECEIVER_LINK: unsafe extern "C" fn(f64, *const PerryPayloadFamily, *mut i32) -> usize =
        js_perry_payload_receiver_link;
    #[used(compiler)]
    static PROTO_GETTER: unsafe extern "C" fn(
        *mut c_void,
        *const u8,
        usize,
        *const crate::closure::JsFunctionInfo,
    ) = js_perry_payload_proto_getter;
    #[used(compiler)]
    static PROTO_DATA: unsafe extern "C" fn(*mut c_void, *const u8, usize, f64, u32) =
        js_perry_payload_proto_data;
    #[used(compiler)]
    static PROTO_INHERIT: unsafe extern "C" fn(*mut c_void, f64) = js_perry_payload_proto_inherit;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    per_test_global! { static DROPS: AtomicUsize = AtomicUsize::new(0); }
    unsafe extern "C" fn drop_bytes(resource: *mut c_void, _: *mut c_void) {
        drop(Box::from_raw(resource as *mut Vec<u8>));
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
    static VTABLE: np::PayloadVTable = np::PayloadVTable {
        drop: drop_bytes,
        stream: None,
    };
    fn family() -> PerryPayloadFamily {
        PerryPayloadFamily {
            abi: payload_abi_layout(),
            class_id: crate::native_class_ids::CRYPTO_HASH,
            links_owner: 1,
            constructor_length: 1,
            _reserved: 0,
            name: b"Bytes".as_ptr(),
            name_len: 5,
            install_prototype: None,
            vtable: &VTABLE,
            payload_size: std::mem::size_of::<Vec<u8>>(),
            payload_align: std::mem::align_of::<Vec<u8>>(),
        }
    }
    #[test]
    fn binding_reopen_preserves_the_stream_record_cell_and_owner_link() {
        DROPS.store(0, Ordering::SeqCst);
        let family = family();
        let scope = crate::gc::RuntimeHandleScope::new();
        let proto = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let owner = scope.root_nanbox_f64(unsafe {
            js_perry_payload_alloc(&family, std::ptr::null_mut(), proto.get_nanbox_f64(), 0)
        });
        unsafe {
            assert_eq!(
                js_perry_payload_lifecycle(owner.get_nanbox_f64(), &family),
                3
            );
            let cell = family_cell(owner.get_nanbox_f64(), &family).unwrap();
            let link = js_perry_payload_owner_link(owner.get_nanbox_f64(), &family);
            assert_eq!(link, cell as usize);
            js_perry_payload_link_ref(link);
            let data = Box::into_raw(Box::new(vec![19u8; 8192]));
            assert_eq!(
                js_perry_payload_reopen(owner.get_nanbox_f64(), &family, data.cast(), 8192),
                0
            );
            crate::node_stream::init_transform_in_place(owner.get_nanbox_f64(), bytes_undefined());
            let obj = np::any_object(owner.get_nanbox_f64()).unwrap();
            let record = (*(*obj).meta).native_state;
            assert!(
                crate::node_stream::is_stream_record_word(record),
                "subject must have a stream state record"
            );
            assert_eq!(family_cell(owner.get_nanbox_f64(), &family), Some(cell));
            assert_eq!(
                js_perry_payload_get(owner.get_nanbox_f64(), &family),
                data.cast()
            );
            assert_eq!((*cell).external_bytes, 8192);
            let refused = Box::into_raw(Box::new(vec![0u8; 1]));
            assert_eq!(
                js_perry_payload_reopen(owner.get_nanbox_f64(), &family, refused.cast(), 1),
                5
            );
            drop(Box::from_raw(refused));
            assert_eq!(js_perry_payload_close(owner.get_nanbox_f64(), &family), 0);
            assert_eq!(DROPS.load(Ordering::SeqCst), 1);
            assert_eq!((*cell).external_bytes, 0);
            assert_eq!((*cell).finalized, 0);
            let mut event_owner = 0.0;
            assert_eq!(js_perry_payload_link_event_owner(link, &mut event_owner), 1);
            assert_eq!(event_owner.to_bits(), owner.get_nanbox_f64().to_bits());
            let next = Box::into_raw(Box::new(vec![7u8; 4096]));
            assert_eq!(
                js_perry_payload_reopen(owner.get_nanbox_f64(), &family, next.cast(), 4096),
                0
            );
            assert_eq!(
                js_perry_payload_owner_link(owner.get_nanbox_f64(), &family),
                link
            );
            let obj = np::any_object(owner.get_nanbox_f64()).unwrap();
            assert_eq!(
                (*(*obj).meta).native_state,
                record,
                "one native_state word; do not replace the stream record"
            );
            assert_eq!(
                js_perry_payload_get(owner.get_nanbox_f64(), &family),
                next.cast()
            );
            js_perry_payload_close(owner.get_nanbox_f64(), &family);
            js_perry_payload_link_unref(link);
            assert_eq!((*cell).refs, 0);
            assert_eq!(DROPS.load(Ordering::SeqCst), 2);
        }
    }
    #[test]
    fn binding_owner_link_and_js_state_use_the_attached_subclass_cell() {
        let family = family();
        let scope = crate::gc::RuntimeHandleScope::new();
        let owner = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(123456, 0) as i64,
        ));
        unsafe {
            let data = Box::into_raw(Box::new(vec![19u8; 4]));
            assert_eq!(
                js_perry_payload_attach(owner.get_nanbox_f64(), &family, data.cast(), 4),
                0
            );
            let cell = family_cell(owner.get_nanbox_f64(), &family).unwrap();
            let link = js_perry_payload_owner_link(owner.get_nanbox_f64(), &family);
            assert_eq!(link, cell as usize);
            let state = scope.root_nanbox_f64(js_perry_payload_js_state(
                owner.get_nanbox_f64(),
                &family,
                1,
            ));
            assert!(np::any_object(state.get_nanbox_f64()).is_some());
            assert_eq!(
                state.get_nanbox_f64().to_bits(),
                js_perry_payload_js_state(owner.get_nanbox_f64(), &family, 0).to_bits()
            );
            assert_eq!(js_perry_payload_close(owner.get_nanbox_f64(), &family), 0);
            assert_eq!(
                js_perry_payload_lifecycle(owner.get_nanbox_f64(), &family),
                3
            );
        }
    }
}

/// The typed cell behind a link, OPEN or CLOSED, on its creator thread.
unsafe fn link_family_cell(
    link: usize,
    family: *const PerryPayloadFamily,
) -> Result<*mut crate::native_handle::NativeHandleHeader, i32> {
    let family = checked_family(family).ok_or(-1)?;
    if link == 0 || link & 7 != 0 {
        return Err(-1);
    }
    let cell = link as *mut crate::native_handle::NativeHandleHeader;
    if (*cell).type_id != family_type_id(family)
        || crate::native_handle::cell_vtable(cell).map(|v| v as *const _) != Some(family.vtable)
    {
        return Err(-1);
    }
    if (*cell).finalized != 0
        || (*cell).creator_thread_id != crate::native_handle::current_thread_id()
    {
        return Err(-2);
    }
    Ok(cell)
}

unsafe fn write_miss(out: *mut i32, code: i32) {
    if !out.is_null() {
        // GC_STORE_AUDIT(POINTER_FREE): a scalar receiver-miss code.
        out.write(code);
    }
}

/// The OPEN payload behind a link; null with `-1` (foreign) or `-2` (closed
/// or finalized) in `out_miss`.
/// # Safety
/// The link's cell is kept alive by a rooted owner or an outstanding ref.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_ptr(
    link: usize,
    family: *const PerryPayloadFamily,
    out_miss: *mut i32,
) -> *mut c_void {
    match link_family_cell(link, family) {
        Ok(cell) => {
            let ptr = crate::native_handle::native_handle_rust_payload_ptr(cell, (*cell).type_id);
            if ptr.is_null() {
                write_miss(out_miss, -2);
            }
            ptr
        }
        Err(code) => {
            write_miss(out_miss, code);
            std::ptr::null_mut()
        }
    }
}

/// Lifecycle of the cell behind a link (codes as `js_perry_payload_lifecycle`).
/// # Safety
/// As js_perry_payload_link_ptr.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_lifecycle(
    link: usize,
    family: *const PerryPayloadFamily,
) -> i32 {
    match link_family_cell(link, family) {
        Ok(cell) if (*cell).flags & np::CLOSING != 0 => 2,
        Ok(cell) if (*cell).resource_ptr.is_null() => 3,
        Ok(_) => 1,
        Err(code) => code,
    }
}

/// Close the payload behind a link, keeping the cell and owner (codes as
/// `js_perry_payload_close`).
/// # Safety
/// As js_perry_payload_link_ptr.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_close(
    link: usize,
    family: *const PerryPayloadFamily,
) -> i32 {
    let Ok(cell) = link_family_cell(link, family) else {
        return -1;
    };
    if (*cell).resource_ptr.is_null() {
        return 0;
    }
    if (*cell).busy != 0 {
        (*cell).flags |= np::CLOSING;
    } else {
        crate::native_handle::native_handle_release_rust_payload(cell);
    }
    0
}

/// Reopen the CLOSED cell behind a link with a new payload (codes as
/// `js_perry_payload_reopen`). Ownership of resource transfers only on 0.
/// # Safety
/// As js_perry_payload_link_ptr; resource is a boxed payload of the family.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_reopen(
    link: usize,
    family_ptr: *const PerryPayloadFamily,
    resource: *mut c_void,
    bytes: usize,
) -> i32 {
    let (Ok(cell), Some(family)) = (
        link_family_cell(link, family_ptr),
        checked_family(family_ptr),
    ) else {
        return -1;
    };
    let Some(owner) = np::link_event_owner(np::OwnerLink(link)) else {
        return 7;
    };
    if resource.is_null() {
        return -1;
    }
    match np::attach_external_cell(
        owner,
        cell,
        family_type_id(family),
        resource,
        &*family.vtable,
        bytes,
    ) {
        Ok(()) => 0,
        Err(np::AttachMiss::Foreign) => -1,
        Err(np::AttachMiss::Open) => 5,
        Err(np::AttachMiss::Closing) => 6,
        Err(np::AttachMiss::Finalized) => 7,
    }
}

/// The owner's traced JS-state object, reached through its link.
/// # Safety
/// As js_perry_payload_link_ptr.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_js_state(
    link: usize,
    family: *const PerryPayloadFamily,
    create: i32,
) -> f64 {
    if link_family_cell(link, family).is_err() {
        return bytes_undefined();
    }
    match np::link_event_owner(np::OwnerLink(link)) {
        Some(owner) => js_perry_payload_js_state(owner, family, create),
        None => bytes_undefined(),
    }
}

/// Restate the native bytes the payload behind a link retains.
/// # Safety
/// As js_perry_payload_link_ptr.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_link_external_bytes(
    link: usize,
    family: *const PerryPayloadFamily,
    bytes: usize,
) {
    if let Ok(cell) = link_family_cell(link, family) {
        crate::native_handle::native_handle_set_external_bytes(cell, bytes);
    }
}

/// The owner link of a receiver of this family (direct or subclass), OPEN or
/// CLOSED; 0 with a miss code otherwise.
/// # Safety
/// family is a static descriptor.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_receiver_link(
    value: f64,
    family_ptr: *const PerryPayloadFamily,
    out_miss: *mut i32,
) -> usize {
    let Some(family) = checked_family(family_ptr) else {
        write_miss(out_miss, -1);
        return 0;
    };
    if family.links_owner == 0 {
        write_miss(out_miss, -1);
        return 0;
    }
    let Some(cell) = family_cell(value, family) else {
        write_miss(out_miss, -1);
        return 0;
    };
    match link_family_cell(cell as usize, family_ptr) {
        Ok(cell) => cell as usize,
        Err(code) => {
            write_miss(out_miss, code);
            0
        }
    }
}

/// Install an accessor on a binding's prototype (inside its installer).
/// # Safety
/// proto is the prototype handed to the installer; name is UTF-8.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_proto_getter(
    proto: *mut c_void,
    name: *const u8,
    name_len: usize,
    info: *const crate::closure::JsFunctionInfo,
) {
    let name = std::str::from_utf8(std::slice::from_raw_parts(name, name_len)).unwrap();
    np::PayloadPrototype::from_raw(proto.cast()).getter(name, info);
}

/// Define a data property on a binding's prototype (inside its installer).
/// flags: bit 0 writable, 1 enumerable, 2 configurable.
/// # Safety
/// As js_perry_payload_proto_getter.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_proto_data(
    proto: *mut c_void,
    name: *const u8,
    name_len: usize,
    value: f64,
    flags: u32,
) {
    let name = std::str::from_utf8(std::slice::from_raw_parts(name, name_len)).unwrap();
    np::PayloadPrototype::from_raw(proto.cast()).data(
        name,
        value,
        flags & 1 != 0,
        flags & 2 != 0,
        flags & 4 != 0,
    );
}

/// Link a binding's prototype to a parent prototype (inside its installer).
/// # Safety
/// As js_perry_payload_proto_getter.
#[no_mangle]
pub unsafe extern "C" fn js_perry_payload_proto_inherit(proto: *mut c_void, parent: f64) {
    np::PayloadPrototype::from_raw(proto.cast()).inherit(parent);
}
