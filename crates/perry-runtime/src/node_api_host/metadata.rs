use super::*;
mod owned;
use crate::value::JSValue;
use owned::with_metadata;
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
    pub module: Option<u32>,
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

mod finalizer_storage {
    use super::*;
    type Value = RefCell<VecDeque<FinalizerRecord>>;
    type Storage = crate::tls_hot::HotCell<Value, { std::mem::needs_drop::<Value>() as usize }>;
    crate::__perry_thread_local_storage!(Storage, const { RefCell::new(VecDeque::new()) });
    pub(super) fn resolve(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
        STORAGE.try_with(|cell| {
            cell.arm_guard(idx);
            cell.value_addr()
        })
    }
    pub(super) static KEY: crate::tls_hot::HotKey<Value> = crate::tls_hot::HotKey::new(
        crate::tls_hot::SlotId::named(concat!(
            module_path!(),
            "::PENDING_FINALIZERS@",
            line!(),
            ":",
            column!()
        )),
        crate::native_finalizer_hook::resolve_finalizer_storage,
    );
}
use finalizer_storage::KEY as PENDING_FINALIZERS;

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
        module: super::modules::current_active_module(),
    })
}

pub(crate) fn cancel_finalizer(owner: usize, id: u64) {
    with_metadata(owner, false, |metadata| {
        metadata.finalizers.retain(|finalizer| finalizer.id != id)
    });
    cancel_pending(id);
}
fn cancel_pending(id: u64) {
    PENDING_FINALIZERS.with(|queue| queue.borrow_mut().retain(|finalizer| finalizer.id != id));
}
pub(crate) fn cancel_finalizer_link(link: u64, id: u64) {
    owned::cancel_link(link, id);
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
    with_metadata(owner, true, |metadata| {
        metadata.external = Some(data as usize);
        metadata.external_finalizer = finalizer(callback, data, hint);
    });
}

pub(crate) fn is_external_owner(owner: usize) -> bool {
    with_metadata(owner, false, |metadata| metadata.external.is_some()).unwrap_or(false)
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
    if PENDING_FINALIZERS
        .try_with(|queue| queue.borrow_mut().extend(callbacks))
        .is_ok()
    {
        crate::event_pump::js_notify_main_thread();
    }
}

pub(crate) fn enqueue_finalizer(finalizer: FinalizerRecord) {
    PENDING_FINALIZERS.with(|queue| queue.borrow_mut().push_back(finalizer));
    crate::event_pump::js_notify_main_thread();
}

pub(crate) fn enqueue_all_object_finalizers() {
    owned::enqueue_all();
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
        with_active_module(env, callback.module, || unsafe {
            (callback.callback)(
                env,
                callback.data as *mut c_void,
                callback.hint as *mut c_void,
            );
        });
        if opened {
            unsafe {
                napi_close_handle_scope(env, scope);
            }
        }
        ran = ran.saturating_add(1);
        settle_callback_exception(env, callback.module, true);
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
    let inserted = with_metadata(owner, true, |metadata| {
        if metadata.wrap.is_some() {
            return false;
        }
        metadata.wrap = Some(NativeWrap {
            data: native_object as usize,
            finalizer: finalizer(finalize_cb, native_object, finalize_hint),
        });
        true
    });
    if inserted != Some(true) {
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
    let data = with_metadata(owner, false, |metadata| {
        metadata.wrap.as_ref().map(|wrap| wrap.data)
    })
    .flatten();
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
    let wrap = with_metadata(owner, false, |metadata| metadata.wrap.take()).flatten();
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
    if with_metadata(owner, true, |metadata| metadata.finalizers.push(finalizer)).is_none() {
        return set_status(
            env,
            NapiStatus::ObjectExpected,
            "value has no owning property holder",
        );
    }
    if !result.is_null() {
        let status = napi_create_reference(env, object, 0, result);
        if status != NapiStatus::Ok {
            if let Some(live_owner) = value_bits(env, object).ok().and_then(owner_from_bits) {
                cancel_finalizer(live_owner, finalizer_id);
            } else {
                cancel_pending(finalizer_id);
            }
            return status;
        }
        let live_owner = value_bits(env, object).ok().and_then(owner_from_bits);
        let link = live_owner.and_then(owned::weak_link);
        if let Some(bits) = link {
            crate::gc::runtime_write_barrier_root_nanbox(bits);
        }
        with_env_mut(env, |env| {
            if let Some(reference) = env.reference_mut(*result) {
                reference.finalizer_link = link.map(|link| (link, finalizer_id));
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
    let data = owner_from_bits(bits)
        .and_then(|owner| with_metadata(owner, false, |metadata| metadata.external).flatten());
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
    let inserted = with_metadata(owner, true, |metadata| {
        if metadata.type_tag.is_some() {
            false
        } else {
            metadata.type_tag = Some(*type_tag);
            true
        }
    });
    if inserted != Some(true) {
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
    *result = with_metadata(owner, false, |metadata| metadata.type_tag)
        .flatten()
        .is_some_and(|stored| stored == *type_tag);
    ok(env)
}

#[cfg(test)]
pub(crate) fn test_seed_owned_metadata(owner: usize) {
    with_metadata(owner, true, |metadata| {
        metadata.wrap = Some(NativeWrap {
            data: 37,
            finalizer: None,
        });
        metadata.type_tag = Some(NapiTypeTag {
            lower: 11,
            upper: 13,
        });
    })
    .expect("fixture owns metadata");
}
#[cfg(test)]
pub(crate) fn test_owned_metadata(owner: usize) -> Option<(usize, u64, u64)> {
    with_metadata(owner, false, |metadata| {
        Some((
            metadata.wrap.as_ref()?.data,
            metadata.type_tag?.lower,
            metadata.type_tag?.upper,
        ))
    })
    .flatten()
}
#[cfg(test)]
pub(crate) fn test_erase_owned_metadata(owner: usize) {
    owned::erase(owner);
}

pub(super) fn install_finalizer_storage() {
    crate::native_finalizer_hook::set_finalizer_storage(finalizer_storage::resolve);
}
