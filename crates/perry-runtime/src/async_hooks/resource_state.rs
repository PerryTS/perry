//! A resource owns its metadata cell. Native schedulers own opaque cell tokens;
//! public ids remain monotonic serial numbers. No id-to-resource registry.
use super::*;

#[repr(C)]
pub(crate) struct ResourceState {
    // An apparent ordinary receiver must never expose a ShapeId at +4.
    _receiver_word: u64,
    pub(super) serial: u64,
    pub(super) type_name: String,
    pub(super) trigger_async_id: u64,
    /// The resource until its destroy notification. A resource destroyed by
    /// its scheduler (timers, immediates, ticks, providers) is held directly:
    /// that edge lives exactly as long as the work is pending, and `forget`
    /// drops it. A promise is destroyed by its own collection, so its state
    /// must not keep it alive and holds a WeakRef to it instead.
    pub(crate) resource: f64,
    pub(crate) context: crate::async_context::AsyncContextSnapshot,
    pub(super) destroyed: bool,
    promise_owner: bool,
    pub(crate) lease_resource: f64,
    native_lease: bool,
}

/// Who keeps a state cell alive.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    /// The resource object (or the promise / native lease): the state is an
    /// edge of its resource.
    Resource,
    /// A scheduler queue entry, then the scope dispatching it: the resource
    /// holds no edge back, so the cell dies with its last entry.
    Scheduler,
}

pub(crate) fn get(id: u64) -> Option<*mut ResourceState> {
    unsafe {
        let header = crate::value::addr_class::try_read_tracked_arena_header_of_type(
            id as usize,
            crate::gc::GC_TYPE_ASYNC_RESOURCE_STATE,
        )?;
        (header.as_ref().size as usize
            == crate::gc::GC_HEADER_SIZE + std::mem::size_of::<ResourceState>())
        .then_some(id as *mut ResourceState)
    }
}

pub(crate) fn public_id(id: u64) -> u64 {
    get(id).map_or(id, |state| unsafe { (*state).serial })
}

pub(super) fn new(
    serial: u64,
    type_name: &str,
    trigger: u64,
    resource: f64,
    context: crate::async_context::AsyncContextSnapshot,
    owner: Owner,
) -> u64 {
    let _publication = crate::gc::GcSuppressScope::new();
    unsafe {
        let resource_value = JSValue::from_bits(resource.to_bits());
        let kind = if resource_value.is_pointer() {
            crate::value::addr_class::try_read_tracked_gc_header(
                resource_value.as_pointer::<u8>() as usize
            )
            .map(|header| header.as_ref().obj_type)
        } else {
            None
        };
        let promise_owner = kind == Some(crate::gc::GC_TYPE_PROMISE);
        // Any other resource value is held as given, heap object or not: a
        // native request's resource can be a non-heap handle value, and
        // `executionAsyncResource()` must still return what init reported.
        let held = if promise_owner {
            crate::value::js_nanbox_pointer(crate::weakref::js_weakref_new(resource) as i64)
        } else {
            resource
        };
        let state = crate::arena::arena_alloc_gc_collecting_holder(
            std::mem::size_of::<ResourceState>(),
            std::mem::align_of::<ResourceState>(),
            crate::gc::GC_TYPE_ASYNC_RESOURCE_STATE,
        )
        .cast::<ResourceState>();
        std::ptr::write(
            state,
            ResourceState {
                _receiver_word: 0,
                serial,
                type_name: type_name.to_string(),
                trigger_async_id: public_id(trigger),
                resource: held,
                context,
                destroyed: false,
                promise_owner,
                lease_resource: TAG_UNDEFINED_F64,
                native_lease: false,
            },
        );
        // `resource` is an inline field of the state cell, traced by its own
        // descriptor: the ordinary slot barrier covers it. Only the snapshot's
        // store words live outside the cell.
        crate::gc::runtime_store_gc_jsvalue_slot(
            state as usize,
            std::ptr::addr_of_mut!((*state).resource) as usize,
            held.to_bits(),
        );
        crate::async_context::for_each_snapshot_store_slot(&mut (*state).context, &mut |slot| {
            crate::gc::runtime_write_barrier_external_slot(
                state as usize,
                slot as usize,
                (*slot).to_bits(),
            );
        });
        if kind == Some(crate::gc::GC_TYPE_PROMISE) {
            let promise = resource_value.as_pointer::<crate::promise::Promise>()
                as *mut crate::promise::Promise;
            crate::gc::runtime_store_gc_heap_word_slot(
                promise as usize,
                std::ptr::addr_of_mut!((*promise).async_id) as usize,
                state as u64,
            );
        } else if kind.is_some() && owner == Owner::Resource {
            crate::object::owned_state_bag::store(
                resource_value.as_pointer::<u8>() as usize,
                crate::object::owned_state_bag::ASYNC_RESOURCE,
                crate::value::js_nanbox_pointer(state as i64),
            );
        }
        state as u64
    }
}

pub(super) fn resource(id: u64) -> f64 {
    unsafe {
        let Some(state) = get(id) else {
            return TAG_UNDEFINED_F64;
        };
        if !(*state).promise_owner {
            return (*state).resource;
        }
        let weak = JSValue::from_bits((*state).resource.to_bits());
        if weak.is_pointer() {
            // This is our own WeakRef holder; a private native read does not
            // create a JavaScript WeakRef keep-alive checkpoint.
            crate::object::js_object_get_field_f64(weak.as_pointer::<ObjectHeader>(), 0)
        } else {
            TAG_UNDEFINED_F64
        }
    }
}

pub(super) fn swap_resource(id: u64, value: f64) -> f64 {
    let _publication = crate::gc::GcSuppressScope::new();
    let previous = resource(id);
    unsafe {
        let Some(state) = get(id) else {
            return previous;
        };
        if !(*state).promise_owner {
            crate::gc::runtime_store_gc_jsvalue_slot(
                state as usize,
                std::ptr::addr_of_mut!((*state).resource) as usize,
                value.to_bits(),
            );
            return previous;
        }
        let weak = JSValue::from_bits((*state).resource.to_bits());
        if weak.is_pointer() {
            crate::object::js_object_set_field_f64(
                weak.as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                0,
                value,
            );
        } else if JSValue::from_bits(value.to_bits()).is_pointer() {
            let weak =
                crate::value::js_nanbox_pointer(crate::weakref::js_weakref_new(value) as i64);
            crate::gc::runtime_store_gc_jsvalue_slot(
                state as usize,
                std::ptr::addr_of_mut!((*state).resource) as usize,
                weak.to_bits(),
            );
        }
    }
    previous
}

/// The separately linked C provider holds this token until destroy. Its pin
/// is the existing native lease root; JavaScript-owned resources are unpinned.
pub(super) fn acquire_native_lease(id: u64, resource: f64) {
    unsafe {
        if let Some(state) = get(id) {
            (*state).native_lease = true;
            crate::gc::runtime_store_gc_jsvalue_slot(
                state as usize,
                std::ptr::addr_of_mut!((*state).lease_resource) as usize,
                resource.to_bits(),
            );
            crate::gc::pin_user_ptr_non_young(state.cast());
        }
    }
}
pub(super) fn forget(id: u64) {
    unsafe {
        if let Some(state) = get(id) {
            drop(std::mem::take(&mut (*state).context));
            (*state).resource = TAG_UNDEFINED_F64;
            (*state).lease_resource = TAG_UNDEFINED_F64;
            if (*state).native_lease {
                (*state).native_lease = false;
                crate::gc::unpin_user_ptr(state.cast());
            }
        }
    }
}

pub(super) fn destroy_notification(
    id: u64,
) -> Option<(u64, bool, Option<crate::async_context::JobDomainToken>)> {
    unsafe {
        let state = get(id)?;
        if (*state).destroyed {
            return None;
        }
        (*state).destroyed = true;
        let notification = (
            (*state).serial,
            (*state).promise_owner,
            (*state).context.job_domain_token(),
        );
        forget(id);
        Some(notification)
    }
}

pub(crate) fn scan_token(id: &mut u64, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if get(*id).is_some() {
        let mut pointer = *id as usize;
        visitor.visit_usize_slot(&mut pointer);
        *id = pointer as u64;
    }
}

/// Native-only idempotent release. Thread teardown skips notifications.
pub(crate) unsafe fn finalize(state: *mut ResourceState, notify: bool) {
    if notify && (*state).promise_owner && !(*state).destroyed {
        super::queue_destroy_notification(
            (*state).serial,
            true,
            (*state).context.job_domain_token(),
        );
    }
    (*state).destroyed = true;
    drop(std::mem::take(&mut (*state).context));
    drop(std::mem::take(&mut (*state).type_name));
    (*state).resource = TAG_UNDEFINED_F64;
    (*state).lease_resource = TAG_UNDEFINED_F64;
    (*state).serial = 0;
}
