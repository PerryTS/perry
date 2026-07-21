//! Per-class metadata registries: parent-class chain, fetch-parent kind,
//! `extends Error`, `Symbol.hasInstance` / `Symbol.toStringTag` hooks
//! (split out of `object/mod.rs`, behavior-preserving).

use std::collections::HashMap;
use std::sync::RwLock;

/// Global class registry mapping class_id -> parent_class_id for inheritance chain lookups
pub(crate) static CLASS_REGISTRY: RwLock<Option<HashMap<u32, u32>>> = RwLock::new(None);

/// class_id -> fetch-builtin parent kind (1 = Request, 2 = Response). Recorded
/// when a class is registered (at module init / class-expression evaluation)
/// whose parent value identifies as the global `Request`/`Response`
/// constructor — including via an alias such as `@hono/node-server`'s
/// `GlobalRequest = global.Request`. Lets the runtime dynamic-construction
/// path (`new (classExprValue)(...)` / ClassRef `new`) attach the underlying
/// native fetch handle, matching what the static codegen `super()` path does.
static FETCH_PARENT_KIND: RwLock<Option<HashMap<u32, u8>>> = RwLock::new(None);

/// Record that `class_id` directly extends the global Request (kind 1) or
/// Response (kind 2) constructor.
pub(crate) fn register_fetch_parent_kind(class_id: u32, kind: u8) {
    let mut g = FETCH_PARENT_KIND.write().unwrap();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g.as_mut().unwrap().insert(class_id, kind);
}

/// The directly-recorded fetch parent kind for `class_id` (no chain walk).
pub(crate) fn fetch_parent_kind(class_id: u32) -> Option<u8> {
    let g = FETCH_PARENT_KIND.read().ok()?;
    g.as_ref()?.get(&class_id).copied()
}

/// Global registry of class IDs that extend the built-in Error class
static EXTENDS_ERROR_REGISTRY: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Per-class `Symbol.hasInstance` static hook. Maps class_id → raw function
/// pointer with signature `extern "C" fn(value: f64) -> f64` (NaN-boxed
/// TAG_TRUE / TAG_FALSE result). Populated at module init from
/// `__perry_wk_hasinstance_<class>` top-level functions lifted by the HIR
/// class lowering.
static CLASS_HAS_INSTANCE_REGISTRY: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Per-class `Symbol.toStringTag` getter hook. Maps class_id → raw function
/// pointer with signature `extern "C" fn(this: f64) -> f64` returning a
/// NaN-boxed STRING_TAG value with the user's tag text. Populated at module
/// init from `__perry_wk_tostringtag_<class>` top-level functions lifted by
/// the HIR class lowering. Consulted by `js_object_to_string` so
/// `Object.prototype.toString.call(x)` returns `[object <tag>]`.
static CLASS_TO_STRING_TAG_REGISTRY: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Register a class-level `Symbol.hasInstance` hook.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_has_instance(class_id: u32, func_ptr: i64) {
    let mut registry = CLASS_HAS_INSTANCE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry
        .as_mut()
        .unwrap()
        .insert(class_id, func_ptr as usize);
}

/// Register a class-level `Symbol.toStringTag` getter hook.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_to_string_tag(class_id: u32, func_ptr: i64) {
    let mut registry = CLASS_TO_STRING_TAG_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry
        .as_mut()
        .unwrap()
        .insert(class_id, func_ptr as usize);
}

pub(crate) fn lookup_has_instance_hook(class_id: u32) -> Option<usize> {
    let reg = CLASS_HAS_INSTANCE_REGISTRY.read().unwrap();
    reg.as_ref().and_then(|m| m.get(&class_id).copied())
}

pub(crate) fn lookup_to_string_tag_hook(class_id: u32) -> Option<usize> {
    let reg = CLASS_TO_STRING_TAG_REGISTRY.read().unwrap();
    reg.as_ref().and_then(|m| m.get(&class_id).copied())
}

/// Mark a user-defined class as extending the built-in Error class.
#[no_mangle]
pub extern "C" fn js_register_class_extends_error(class_id: u32) {
    let mut registry = EXTENDS_ERROR_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(std::collections::HashSet::new());
    }
    registry.as_mut().unwrap().insert(class_id);
}

/// Check if a class id extends the built-in Error class
pub(crate) fn extends_builtin_error(class_id: u32) -> bool {
    let registry = EXTENDS_ERROR_REGISTRY.read().unwrap();
    if let Some(reg) = registry.as_ref() {
        if reg.contains(&class_id) {
            return true;
        }
        let mut current = class_id;
        let parent_reg = CLASS_REGISTRY.read().unwrap();
        if let Some(pr) = parent_reg.as_ref() {
            for _ in 0..32 {
                match pr.get(&current).copied() {
                    Some(parent) if parent != 0 => {
                        if reg.contains(&parent) {
                            return true;
                        }
                        current = parent;
                    }
                    _ => break,
                }
            }
        }
    }
    false
}
