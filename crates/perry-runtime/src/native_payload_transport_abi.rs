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
