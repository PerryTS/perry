use super::*;
use crate::value::JSValue;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

pub type NapiFinalize = Option<unsafe extern "C" fn(NapiEnv, *mut c_void, *mut c_void)>;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NapiTypeTag {
    pub lower: u64,
    pub upper: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct FinalizerRecord {
    pub id: u64,
    pub callback: unsafe extern "C" fn(NapiEnv, *mut c_void, *mut c_void),
    pub data: usize,
    pub hint: usize,
}

static NEXT_FINALIZER_ID: AtomicU64 = AtomicU64::new(1);

struct NativeWrap {
    data: usize,
    finalizer: Option<FinalizerRecord>,
}

#[derive(Default)]
struct ObjectMetadata {
    wrap: Option<NativeWrap>,
    external: Option<usize>,
    external_finalizer: Option<FinalizerRecord>,
    finalizers: Vec<FinalizerRecord>,
    type_tag: Option<NapiTypeTag>,
}

crate::perry_thread_local! {
    static OBJECT_METADATA: RefCell<crate::fast_hash::PtrHashMap<usize, ObjectMetadata>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    static PENDING_FINALIZERS: RefCell<VecDeque<FinalizerRecord>> =
        const { RefCell::new(VecDeque::new()) };
}

pub(crate) fn finalizer(
    callback: NapiFinalize,
    data: *mut c_void,
    hint: *mut c_void,
) -> Option<FinalizerRecord> {
    callback.map(|callback| FinalizerRecord {
        id: NEXT_FINALIZER_ID.fetch_add(1, Ordering::Relaxed),
        callback,
        data: data as usize,
        hint: hint as usize,
    })
}

pub(crate) fn cancel_finalizer(owner: usize, id: u64) {
    OBJECT_METADATA.with(|table| {
        if let Some(metadata) = table.borrow_mut().get_mut(&owner) {
            metadata.finalizers.retain(|finalizer| finalizer.id != id);
        }
    });
    PENDING_FINALIZERS.with(|queue| {
        queue.borrow_mut().retain(|finalizer| finalizer.id != id);
    });
}

pub(crate) fn owner_from_bits(bits: u64) -> Option<usize> {
    let value = JSValue::from_bits(bits);
    if !value.is_pointer() {
        return None;
    }
    let owner = value.as_pointer::<u8>() as usize;
    (owner >= 0x10000).then_some(owner)
}

fn object_owner(env: NapiEnv, value: NapiValue) -> Result<usize, NapiStatus> {
    let bits = value_bits(env, value)?;
    let object_like =
        unsafe { crate::object::object_ops::value_is_object_like(f64::from_bits(bits)) };
    if !object_like {
        return Err(NapiStatus::ObjectExpected);
    }
    owner_from_bits(bits).ok_or(NapiStatus::ObjectExpected)
}

pub(crate) fn attach_external_finalizer(
    owner: usize,
    data: *mut c_void,
    callback: NapiFinalize,
    hint: *mut c_void,
) {
    OBJECT_METADATA.with(|table| {
        let mut table = table.borrow_mut();
        let metadata = table.entry(owner).or_default();
        metadata.external = Some(data as usize);
        metadata.external_finalizer = finalizer(callback, data, hint);
    });
}

pub(crate) fn attach_owner_finalizer(
    owner: usize,
    data: *mut c_void,
    callback: NapiFinalize,
    hint: *mut c_void,
) {
    let Some(finalizer) = finalizer(callback, data, hint) else {
        return;
    };
    OBJECT_METADATA.with(|table| {
        table
            .borrow_mut()
            .entry(owner)
            .or_default()
            .finalizers
            .push(finalizer);
    });
}

pub(crate) fn is_external_owner(owner: usize) -> bool {
    OBJECT_METADATA.with(|table| {
        table
            .borrow()
            .get(&owner)
            .is_some_and(|metadata| metadata.external.is_some())
    })
}

fn enqueue_metadata_finalizers(mut metadata: ObjectMetadata) {
    let mut callbacks = Vec::new();
    if let Some(wrap) = metadata.wrap.take() {
        if let Some(finalizer) = wrap.finalizer {
            callbacks.push(finalizer);
        }
    }
    if let Some(finalizer) = metadata.external_finalizer.take() {
        callbacks.push(finalizer);
    }
    callbacks.append(&mut metadata.finalizers);
    if callbacks.is_empty() {
        return;
    }
    PENDING_FINALIZERS.with(|queue| queue.borrow_mut().extend(callbacks));
    crate::event_pump::js_notify_main_thread();
}

pub(crate) fn enqueue_finalizer(finalizer: FinalizerRecord) {
    PENDING_FINALIZERS.with(|queue| queue.borrow_mut().push_back(finalizer));
    crate::event_pump::js_notify_main_thread();
}

pub(crate) fn enqueue_all_object_finalizers() {
    let all = OBJECT_METADATA.with(|table| {
        std::mem::take(&mut *table.borrow_mut())
            .into_values()
            .collect::<Vec<_>>()
    });
    for metadata in all {
        enqueue_metadata_finalizers(metadata);
    }
}

pub(crate) fn scan_object_metadata_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    OBJECT_METADATA.with(|table| {
        let old = std::mem::take(&mut *table.borrow_mut());
        let mut rebuilt = crate::fast_hash::new_ptr_hash_map();
        for (owner, metadata) in old {
            let mut new_owner = owner;
            visitor.visit_metadata_usize_slot(&mut new_owner);
            rebuilt.insert(new_owner, metadata);
        }
        *table.borrow_mut() = rebuilt;
    });
}

/// GC dead-owner registry callback. Removal is the one point that schedules
/// finalization, so moving collections can re-key live owners without rooting
/// them and both copying and mark/sweep paths have identical behavior.
pub(crate) fn prune_dead_object_meta_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    let removed = OBJECT_METADATA.with(|table| {
        let mut table = table.borrow_mut();
        let owners: Vec<_> = table
            .keys()
            .copied()
            .filter(|owner| is_dead_owner(*owner))
            .collect();
        owners
            .into_iter()
            .filter_map(|owner| table.remove(&owner))
            .collect::<Vec<_>>()
    });
    for metadata in removed {
        enqueue_metadata_finalizers(metadata);
    }
}

pub(crate) fn drain_pending_finalizers() -> i32 {
    let mut ran = 0i32;
    loop {
        let callback = PENDING_FINALIZERS.with(|queue| queue.borrow_mut().pop_front());
        let Some(callback) = callback else {
            break;
        };
        let env = current_env();
        let mut scope = std::ptr::null_mut();
        let opened = unsafe { napi_open_handle_scope(env, &mut scope) } == NapiStatus::Ok;
        unsafe {
            (callback.callback)(
                env,
                callback.data as *mut c_void,
                callback.hint as *mut c_void,
            );
        }
        if opened {
            unsafe {
                napi_close_handle_scope(env, scope);
            }
        }
        ran = ran.saturating_add(1);
    }
    ran
}

pub(crate) fn has_pending_finalizers() -> bool {
    PENDING_FINALIZERS.with(|queue| !queue.borrow().is_empty())
}

#[no_mangle]
pub unsafe extern "C" fn napi_wrap(
    env: NapiEnv,
    object: NapiValue,
    native_object: *mut c_void,
    finalize_cb: NapiFinalize,
    finalize_hint: *mut c_void,
    result: *mut NapiRef,
) -> NapiStatus {
    let owner = match object_owner(env, object) {
        Ok(owner) => owner,
        Err(status) => return set_status(env, status, "value must be an object"),
    };
    let inserted = OBJECT_METADATA.with(|table| {
        let mut table = table.borrow_mut();
        let metadata = table.entry(owner).or_default();
        if metadata.wrap.is_some() {
            return false;
        }
        metadata.wrap = Some(NativeWrap {
            data: native_object as usize,
            finalizer: finalizer(finalize_cb, native_object, finalize_hint),
        });
        true
    });
    if !inserted {
        return set_status(env, NapiStatus::InvalidArg, "object is already wrapped");
    }
    if !result.is_null() {
        let status = napi_create_reference(env, object, 0, result);
        if status != NapiStatus::Ok {
            return status;
        }
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_unwrap(
    env: NapiEnv,
    object: NapiValue,
    result: *mut *mut c_void,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let owner = match object_owner(env, object) {
        Ok(owner) => owner,
        Err(status) => return set_status(env, status, "value must be an object"),
    };
    let data = OBJECT_METADATA.with(|table| {
        table
            .borrow()
            .get(&owner)
            .and_then(|metadata| metadata.wrap.as_ref().map(|wrap| wrap.data))
    });
    let Some(data) = data else {
        return set_status(env, NapiStatus::InvalidArg, "object is not wrapped");
    };
    *result = data as *mut c_void;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_remove_wrap(
    env: NapiEnv,
    object: NapiValue,
    result: *mut *mut c_void,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let owner = match object_owner(env, object) {
        Ok(owner) => owner,
        Err(status) => return set_status(env, status, "value must be an object"),
    };
    let wrap = OBJECT_METADATA.with(|table| {
        table
            .borrow_mut()
            .get_mut(&owner)
            .and_then(|metadata| metadata.wrap.take())
    });
    let Some(wrap) = wrap else {
        return set_status(env, NapiStatus::InvalidArg, "object is not wrapped");
    };
    *result = wrap.data as *mut c_void;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_add_finalizer(
    env: NapiEnv,
    object: NapiValue,
    finalize_data: *mut c_void,
    finalize_cb: NapiFinalize,
    finalize_hint: *mut c_void,
    result: *mut NapiRef,
) -> NapiStatus {
    let Some(finalizer) = finalizer(finalize_cb, finalize_data, finalize_hint) else {
        return set_status(env, NapiStatus::InvalidArg, "finalizer must not be null");
    };
    let owner = match object_owner(env, object) {
        Ok(owner) => owner,
        Err(status) => return set_status(env, status, "value must be an object"),
    };
    let finalizer_id = finalizer.id;
    OBJECT_METADATA.with(|table| {
        table
            .borrow_mut()
            .entry(owner)
            .or_default()
            .finalizers
            .push(finalizer);
    });
    if !result.is_null() {
        let status = napi_create_reference(env, object, 0, result);
        if status != NapiStatus::Ok {
            cancel_finalizer(owner, finalizer_id);
            return status;
        }
        with_env_mut(env, |env| {
            if let Some(reference) = env.reference_mut(*result) {
                reference.finalizer_link = Some((owner, finalizer_id));
            }
        });
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_external(
    env: NapiEnv,
    value: NapiValue,
    result: *mut *mut c_void,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let bits = match value_bits(env, value) {
        Ok(bits) => bits,
        Err(status) => return set_status(env, status, "value is not a live handle"),
    };
    let data = owner_from_bits(bits).and_then(|owner| {
        OBJECT_METADATA.with(|table| {
            table
                .borrow()
                .get(&owner)
                .and_then(|metadata| metadata.external)
        })
    });
    let Some(data) = data else {
        return set_status(env, NapiStatus::InvalidArg, "value is not an external");
    };
    *result = data as *mut c_void;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_type_tag_object(
    env: NapiEnv,
    value: NapiValue,
    type_tag: *const NapiTypeTag,
) -> NapiStatus {
    if type_tag.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "type tag must not be null");
    }
    let owner = match object_owner(env, value) {
        Ok(owner) => owner,
        Err(status) => return set_status(env, status, "value must be an object"),
    };
    let inserted = OBJECT_METADATA.with(|table| {
        let mut table = table.borrow_mut();
        let metadata = table.entry(owner).or_default();
        if metadata.type_tag.is_some() {
            false
        } else {
            metadata.type_tag = Some(*type_tag);
            true
        }
    });
    if !inserted {
        return set_status(env, NapiStatus::InvalidArg, "object already has a type tag");
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_check_object_type_tag(
    env: NapiEnv,
    value: NapiValue,
    type_tag: *const NapiTypeTag,
    result: *mut bool,
) -> NapiStatus {
    if type_tag.is_null() || result.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "type tag and result must not be null",
        );
    }
    let owner = match object_owner(env, value) {
        Ok(owner) => owner,
        Err(status) => return set_status(env, status, "value must be an object"),
    };
    *result = OBJECT_METADATA.with(|table| {
        table
            .borrow()
            .get(&owner)
            .and_then(|metadata| metadata.type_tag)
            .is_some_and(|stored| stored == *type_tag)
    });
    ok(env)
}
