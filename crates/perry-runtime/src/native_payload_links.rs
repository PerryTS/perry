//! Reference pins and acceptance census for stable payload links.
use super::*;

/// Default-off acceptance instrumentation for net.Socket payload cells.
/// Scalar counts only; no addresses or identities are stored.
#[cfg(feature = "native-payload-test-census")]
#[path = "native_payload_census.rs"]
pub mod test_census;

/// # Safety
/// Call on the creator thread with a live cell, once per outstanding item.
pub unsafe fn link_ref(link: OwnerLink) {
    let cell = link.0 as *mut NativeHandleHeader;
    assert_eq!(
        (*cell).creator_thread_id,
        crate::native_handle::current_thread_id()
    );
    (*cell).refs = (*cell).refs.checked_add(1).expect("native refs overflow");
    #[cfg(feature = "native-payload-test-census")]
    test_census::reference((*cell).type_id);
    #[cfg(test)]
    if callback_sabotage("pin") {
        return;
    }
    if (*cell).refs == 1 {
        #[cfg(test)]
        if callback_sabotage("latch") {
            crate::gc::pin_object(
                crate::value::addr_class::try_read_tracked_gc_header(
                    ((*cell).owner & crate::value::POINTER_MASK) as usize,
                )
                .expect("live payload owner")
                .as_ptr(),
            );
            return;
        }
        crate::gc::pin_object_non_young(
            crate::value::addr_class::try_read_tracked_gc_header(cell as usize)
                .expect("live payload cell")
                .as_ptr(),
        );
    }
}

/// # Safety
/// Match a link_ref on the creator thread, even when explicitly closed.
pub unsafe fn link_unref(link: OwnerLink) {
    let cell = link.0 as *mut NativeHandleHeader;
    assert_eq!(
        (*cell).creator_thread_id,
        crate::native_handle::current_thread_id()
    );
    assert_ne!((*cell).refs, 0, "unbalanced native unref");
    (*cell).refs -= 1;
    #[cfg(feature = "native-payload-test-census")]
    test_census::unreference((*cell).type_id);
    if (*cell).refs == 0 {
        crate::gc::unpin_object(
            crate::value::addr_class::try_read_tracked_gc_header(cell as usize)
                .expect("live payload cell")
                .as_ptr(),
        );
    }
}
