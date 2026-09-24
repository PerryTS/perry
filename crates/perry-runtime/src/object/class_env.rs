//! Guarded class capture environments (`Expr::ClassEnvGet { guarded: true }`).
//!
//! A capturing class EXPRESSION inside a function body evaluates to a fresh
//! class object per evaluation, each carrying its own capture array
//! (`__perry_ctor_caps`). Its members read their captures from one set of
//! module-state slot globals owned by the class's FIRST evaluation. While the
//! class has had only that evaluation, the slots are the whole truth and
//! generated code reads them directly after one compare of the class's state
//! global against 0. A second evaluation — a CommonJS module body the runtime
//! re-runs (`module_require.rs`'s re-require of a loaded module) — sets the
//! state to 1; from then on generated code calls into this module, which
//! resolves the receiver's own evaluation and reads that evaluation's array
//! unless it is the owner.
//!
//! An instance names its evaluation through the private-evaluation brand in
//! its metadata record: dynamic construction stamps it always, and a static
//! `new` stamps it (`js_class_env_stamp`) once the class is in the multi-
//! evaluation state. An unstamped instance belongs to the first evaluation.

use std::collections::HashMap;

use crate::value::TAG_UNDEFINED;

struct ClassEnv {
    /// NaN-boxed class object of the first evaluation (0 = none yet). A GC
    /// root: see [`scan_class_env_roots_mut`].
    owner: u64,
    /// The class's state global (`0.0` = one evaluation, `1.0` = several).
    state: *mut f64,
    /// The slot globals, by capture index. Registered GC roots of their own.
    slots: Vec<*mut f64>,
}

crate::perry_thread_local! {
    static CLASS_ENVS: std::cell::RefCell<HashMap<u32, ClassEnv>> =
        std::cell::RefCell::new(HashMap::new());
}

fn with_env<R>(cid: u32, f: impl FnOnce(&mut ClassEnv) -> R) -> R {
    CLASS_ENVS.with(|envs| {
        let mut envs = envs.borrow_mut();
        let env = envs.entry(cid).or_insert_with(|| ClassEnv {
            owner: 0,
            state: std::ptr::null_mut(),
            slots: Vec::new(),
        });
        f(env)
    })
}

/// GC root scan for the owner class objects.
pub fn scan_class_env_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CLASS_ENVS.with(|envs| {
        for env in envs.borrow_mut().values_mut() {
            if env.owner != 0 {
                visitor.visit_nanbox_u64_slot(&mut env.owner);
            }
        }
    });
}

/// Codegen FFI (module init): the class's state global.
#[no_mangle]
pub unsafe extern "C" fn js_class_env_register_state(cid: u32, state: *mut f64) {
    with_env(cid, |env| env.state = state);
}

/// Codegen FFI (module init): slot global `index` of the class environment.
#[no_mangle]
pub unsafe extern "C" fn js_class_env_register_slot(cid: u32, index: u32, slot: *mut f64) {
    with_env(cid, |env| {
        let index = index as usize;
        if env.slots.len() <= index {
            env.slots.resize(index + 1, std::ptr::null_mut());
        }
        env.slots[index] = slot;
    });
}

/// Store `value` into a registered root slot, with the incremental-mark root
/// barrier generated code uses for the same slots.
unsafe fn store_slot(slot: *mut f64, value: f64) {
    if slot.is_null() {
        return;
    }
    *slot = value;
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
}

/// Copy a capture array into the owner's slots.
unsafe fn publish_array(cid: u32, caps: f64) {
    let caps = crate::value::JSValue::from_bits(caps.to_bits());
    if !caps.is_pointer() {
        return;
    }
    let array = caps.as_pointer::<crate::array::ArrayHeader>();
    if array.is_null() {
        return;
    }
    let len = crate::array::js_array_length(array);
    let slots: Vec<*mut f64> = with_env(cid, |env| env.slots.clone());
    for (index, slot) in slots.into_iter().enumerate() {
        if (index as u32) < len {
            store_slot(slot, crate::array::js_array_get_f64(array, index as u32));
        }
    }
}

/// Codegen FFI: a guarded class evaluated, producing `class_value` with the
/// capture array `caps`. The first evaluation becomes the owner and publishes
/// its captures into the slots; any other one only moves the class into the
/// multi-evaluation state (its members then resolve per receiver).
#[no_mangle]
pub unsafe extern "C" fn js_class_env_evaluate(cid: u32, class_value: f64, caps: f64) {
    let owner = with_env(cid, |env| {
        if env.owner == 0 {
            env.owner = class_value.to_bits();
            true
        } else if env.owner == class_value.to_bits() {
            true
        } else {
            if !env.state.is_null() {
                *env.state = 1.0;
            }
            false
        }
    });
    if owner {
        publish_array(cid, caps);
    }
}

/// Codegen FFI: `class_value`'s capture array was refreshed (a captured
/// binding was assigned after the class evaluated). Only the owner's refresh
/// reaches the slots.
#[no_mangle]
pub unsafe extern "C" fn js_class_env_refresh(cid: u32, class_value: f64, caps: f64) {
    if with_env(cid, |env| env.owner == class_value.to_bits()) {
        publish_array(cid, caps);
    }
}

/// The evaluation a member of class `cid` runs in: the method value's own
/// evaluation (extracted-method dispatch), else the receiver's.
fn member_evaluation(receiver: f64, cid: u32) -> Option<u64> {
    super::field_get_set::current_private_lexical_brand_value(cid)
        .or_else(|| super::field_get_set::class_evaluation_of(receiver, cid))
        .map(f64::to_bits)
}

/// The capture array of a non-owner evaluation, or `None` for the owner.
fn foreign_caps(receiver: f64, cid: u32) -> Option<*mut crate::array::ArrayHeader> {
    let evaluation = member_evaluation(receiver, cid)?;
    if with_env(cid, |env| env.owner == evaluation) {
        return None;
    }
    let caps = super::js_object_get_own_field_or_undef(
        f64::from_bits(evaluation),
        b"__perry_ctor_caps".as_ptr(),
        17,
    );
    let caps = crate::value::JSValue::from_bits(caps.to_bits());
    if !caps.is_pointer() {
        return None;
    }
    let array = caps.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
    (!array.is_null()).then_some(array)
}

/// Codegen FFI: the slow read, taken once the class has several evaluations.
#[no_mangle]
pub unsafe extern "C" fn js_class_env_get(receiver: f64, cid: u32, index: u32) -> f64 {
    if let Some(array) = foreign_caps(receiver, cid) {
        return if index < crate::array::js_array_length(array) {
            crate::array::js_array_get_f64(array, index)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
    }
    let slot = with_env(cid, |env| {
        env.slots
            .get(index as usize)
            .copied()
            .unwrap_or(std::ptr::null_mut())
    });
    if slot.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        *slot
    }
}

/// Codegen FFI: the slow write of a captured binding (see `js_class_env_get`).
#[no_mangle]
pub unsafe extern "C" fn js_class_env_set(receiver: f64, cid: u32, index: u32, value: f64) {
    if let Some(array) = foreign_caps(receiver, cid) {
        if index < crate::array::js_array_length(array) {
            crate::array::js_array_set_f64(array, index, value);
        }
        return;
    }
    let slot = with_env(cid, |env| {
        env.slots
            .get(index as usize)
            .copied()
            .unwrap_or(std::ptr::null_mut())
    });
    store_slot(slot, value);
}

/// Codegen FFI: the evaluation (class value) a member of class `cid` runs in,
/// or `undefined` when it cannot be resolved — which, like an unrecorded
/// instance, means the first evaluation.
#[no_mangle]
pub extern "C" fn js_class_env_current(receiver: f64, cid: u32) -> f64 {
    member_evaluation(receiver, cid)
        .map(f64::from_bits)
        .unwrap_or(f64::from_bits(TAG_UNDEFINED))
}

/// Codegen FFI: record `evaluation` as the evaluation of `instance`, built by
/// a static `new` of class `cid` while the class has several evaluations.
/// Returns the instance (the metadata allocation may move it).
#[no_mangle]
pub unsafe extern "C" fn js_class_env_stamp(instance: f64, cid: u32, evaluation: f64) -> f64 {
    if with_env(cid, |env| env.owner == evaluation.to_bits())
        || !super::class_registry::is_class_object_value(evaluation)
    {
        return instance;
    }
    let value = crate::value::JSValue::from_bits(instance.to_bits());
    if !value.is_pointer() {
        return instance;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(instance);
    let object = value.as_pointer::<super::ObjectHeader>() as *mut super::ObjectHeader;
    if object.is_null() {
        return instance;
    }
    super::field_get_set::stamp_private_evaluation_brand(object, evaluation);
    handle.get_nanbox_f64()
}

#[cfg(feature = "keepalive-anchors")]
mod keepalive {
    #[used(compiler)]
    static REGISTER_STATE: unsafe extern "C" fn(u32, *mut f64) = super::js_class_env_register_state;
    #[used(compiler)]
    static REGISTER_SLOT: unsafe extern "C" fn(u32, u32, *mut f64) =
        super::js_class_env_register_slot;
    #[used(compiler)]
    static EVALUATE: unsafe extern "C" fn(u32, f64, f64) = super::js_class_env_evaluate;
    #[used(compiler)]
    static REFRESH: unsafe extern "C" fn(u32, f64, f64) = super::js_class_env_refresh;
    #[used(compiler)]
    static GET: unsafe extern "C" fn(f64, u32, u32) -> f64 = super::js_class_env_get;
    #[used(compiler)]
    static SET: unsafe extern "C" fn(f64, u32, u32, f64) = super::js_class_env_set;
    #[used(compiler)]
    static STAMP: unsafe extern "C" fn(f64, u32, f64) -> f64 = super::js_class_env_stamp;
    #[used(compiler)]
    static CURRENT: extern "C" fn(f64, u32) -> f64 = super::js_class_env_current;
}
