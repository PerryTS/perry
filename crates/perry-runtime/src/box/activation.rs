//! Stable generation tokens reject stale asynchronous resumes. Cells themselves
//! are ordinary traced GC objects and are not owned by these tokens.
crate::perry_thread_local! {
    static NEXT_ASYNC_BOX_ACTIVATION_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static ASYNC_BOX_ACTIVATION_FREE_HEAD: std::cell::Cell<*mut AsyncBoxActivation> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}
/// Malloc-side reachability token for one lowered plain-async activation.
///
/// `refs` counts the lifecycle owner plus queued/running async-step owners.
/// At zero the token can be reused with a new generation. It contains no
/// GC pointers and has no role in cell liveness.
pub(crate) struct AsyncBoxActivation {
    id: u64,
    refs: std::cell::Cell<usize>,
    lifecycle_owned: std::cell::Cell<bool>,
    next_free: std::cell::Cell<*mut AsyncBoxActivation>,
}

/// Create the stable token for a plain-async activation. The activation
/// lifecycle owns the initial reference until a terminal release (or
/// `js_async_step_done`) marks the activation complete.
pub(crate) fn new_async_box_activation() -> *mut AsyncBoxActivation {
    let id = NEXT_ASYNC_BOX_ACTIVATION_ID.with(|next| {
        let id = next.get();
        // IDs are stored losslessly in a closure's f64 capture. Reaching 2^53
        // activations in one thread is not realistic; wrapping to 1 keeps 0 as
        // the permanent "not a tracked plain-async activation" sentinel.
        let following = if id >= (1u64 << 53) - 1 { 1 } else { id + 1 };
        next.set(following);
        id
    });
    let ptr = ASYNC_BOX_ACTIVATION_FREE_HEAD.with(|head| {
        let ptr = head.get();
        if ptr.is_null() {
            std::boxed::Box::into_raw(std::boxed::Box::new(AsyncBoxActivation {
                id,
                refs: std::cell::Cell::new(1),
                lifecycle_owned: std::cell::Cell::new(true),
                next_free: std::cell::Cell::new(std::ptr::null_mut()),
            }))
        } else {
            unsafe {
                head.set((*ptr).next_free.get());
                (*ptr).id = id;
                (*ptr).refs.set(1);
                (*ptr).lifecycle_owned.set(true);
                (*ptr).next_free.set(std::ptr::null_mut());
            }
            ptr
        }
    });
    ptr
}

#[inline]
pub(crate) fn async_box_activation_id(ptr: *mut AsyncBoxActivation) -> u64 {
    if ptr.is_null() {
        0
    } else {
        unsafe { (*ptr).id }
    }
}

/// Validate the stable token pointer + generation captured by a pending-await
/// thunk. Token storage is never freed, so reading it is safe even after the
/// token was recycled; the generation and lifecycle bit reject that stale
/// capture without a HashMap lookup on every async activation.
pub(crate) fn find_async_box_activation(
    ptr: *mut AsyncBoxActivation,
    id: u64,
) -> *mut AsyncBoxActivation {
    if ptr.is_null() || id == 0 {
        return std::ptr::null_mut();
    }
    unsafe {
        if (*ptr).id == id && (*ptr).lifecycle_owned.get() && (*ptr).refs.get() > 0 {
            ptr
        } else {
            std::ptr::null_mut()
        }
    }
}

#[inline]
pub(crate) fn retain_async_box_activation(ptr: *mut AsyncBoxActivation) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let old = (*ptr).refs.get();
        debug_assert!(old > 0);
        (*ptr).refs.set(
            old.checked_add(1)
                .expect("async activation refcount overflow"),
        );
    }
}

#[inline]
pub(crate) fn release_async_box_activation(ptr: *mut AsyncBoxActivation) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let old = (*ptr).refs.get();
        debug_assert!(old > 0);
        let new = old - 1;
        (*ptr).refs.set(new);
        if new == 0 {
            debug_assert!(!(*ptr).lifecycle_owned.get());
            ASYNC_BOX_ACTIVATION_FREE_HEAD.with(|head| {
                (*ptr).next_free.set(head.get());
                head.set(ptr);
            });
        }
    }
}

pub(crate) fn finish_async_box_activation(ptr: *mut AsyncBoxActivation) {
    if ptr.is_null() {
        return;
    }
    if unsafe { (*ptr).lifecycle_owned.replace(false) } {
        release_async_box_activation(ptr);
    }
}
