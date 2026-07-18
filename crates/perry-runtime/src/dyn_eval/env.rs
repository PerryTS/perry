//! Interpreter environments (#6559).
//!
//! A scope is an ordinary runtime object (null prototype, class_id 0):
//! variable name → value as properties, parent scope under a key that can
//! never collide with a JS identifier. Environments therefore live in the
//! normal GC object graph — an interpreted closure keeps its whole defining
//! chain alive through one traced capture slot, and moving collections
//! relocate scopes like any other object (no Rust-side pointer can go stale
//! because every held value routes through the rooted stack in `mod.rs`).

use super::{root_get, root_push, root_set, roots_truncate};

/// Parent-scope key. Contains a space, so no declared identifier can ever
/// shadow or collide with it (interpreted code only reaches environments via
/// identifier resolution, never via computed access).
const PARENT_KEY: &str = "perry dyn parent";

fn key_string(name: &str) -> *mut crate::string::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn env_object_ptr(env: f64) -> *mut crate::object::ObjectHeader {
    crate::value::js_nanbox_get_pointer(env) as *mut crate::object::ObjectHeader
}

/// Allocate a fresh scope with no parent (a Function instance's root scope —
/// also the target of sloppy assignments to undeclared names).
pub(crate) fn env_new_root() -> f64 {
    let obj = crate::object::js_object_alloc_null_proto(0, 0);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Allocate a fresh scope chained to `parent` (rooted by the caller).
pub(crate) fn env_new(parent: f64) -> f64 {
    let parent_idx = root_push(parent);
    let obj = crate::object::js_object_alloc_null_proto(0, 0);
    let env = crate::value::js_nanbox_pointer(obj as i64);
    let env_idx = root_push(env);
    let key = key_string(PARENT_KEY);
    crate::object::js_object_set_field_by_name(
        env_object_ptr(root_get(env_idx)),
        key,
        root_get(parent_idx),
    );
    let env = root_get(env_idx);
    roots_truncate(parent_idx);
    env
}

fn env_parent(env: f64) -> Option<f64> {
    let env_idx = root_push(env);
    let key = key_string(PARENT_KEY);
    let value =
        crate::object::js_object_get_field_by_name(env_object_ptr(root_get(env_idx)), key);
    roots_truncate(env_idx);
    let bits = value.bits();
    let v = f64::from_bits(bits);
    if crate::value::JSValue::from_bits(bits).is_undefined() {
        None
    } else {
        Some(v)
    }
}

fn env_has_own(env: f64, name: &str) -> bool {
    let env_idx = root_push(env);
    let key = key_string(name);
    let key_value = crate::value::js_nanbox_string(key as i64);
    let has = crate::object::js_object_has_own(root_get(env_idx), key_value);
    roots_truncate(env_idx);
    crate::value::js_is_truthy(has) != 0
}

fn env_read(env: f64, name: &str) -> f64 {
    let env_idx = root_push(env);
    let key = key_string(name);
    let value =
        crate::object::js_object_get_field_by_name(env_object_ptr(root_get(env_idx)), key);
    roots_truncate(env_idx);
    f64::from_bits(value.bits())
}

fn env_write(env: f64, name: &str, value: f64) {
    let env_idx = root_push(env);
    let value_idx = root_push(value);
    let key = key_string(name);
    crate::object::js_object_set_field_by_name(
        env_object_ptr(root_get(env_idx)),
        key,
        root_get(value_idx),
    );
    roots_truncate(env_idx);
}

/// Declare `name` in exactly this scope (let/const/var-hoist/param/function).
pub(crate) fn define(env: f64, name: &str, value: f64) {
    env_write(env, name, value);
}

/// Read `name`, walking the scope chain. `None` when no scope binds it (the
/// caller then falls back to the real `globalThis`).
///
/// The cursor lives in a rooted slot: `env_has_own` / `env_parent` allocate
/// key strings, and a moving collection triggered by those allocations would
/// otherwise leave a raw `f64` cursor stale.
pub(crate) fn lookup(env: f64, name: &str) -> Option<f64> {
    let cur_idx = root_push(env);
    loop {
        if env_has_own(root_get(cur_idx), name) {
            let value = env_read(root_get(cur_idx), name);
            roots_truncate(cur_idx);
            return Some(value);
        }
        match env_parent(root_get(cur_idx)) {
            Some(p) => root_set(cur_idx, p),
            None => {
                roots_truncate(cur_idx);
                return None;
            }
        }
    }
}

/// Whether any scope in the chain binds `name`.
pub(crate) fn is_bound(env: f64, name: &str) -> bool {
    let cur_idx = root_push(env);
    loop {
        if env_has_own(root_get(cur_idx), name) {
            roots_truncate(cur_idx);
            return true;
        }
        match env_parent(root_get(cur_idx)) {
            Some(p) => root_set(cur_idx, p),
            None => {
                roots_truncate(cur_idx);
                return false;
            }
        }
    }
}

/// Assign `name = value`: writes the nearest binding scope, or — sloppy-mode
/// semantics, which `new Function` bodies get in Node and which find-my-way's
/// generated matcher relies on (`value = derivedConstraints.version` with
/// `value` never declared) — creates the binding on the chain's ROOT scope
/// (the Function instance's private "global").
pub(crate) fn assign(env: f64, name: &str, value: f64) {
    let value_idx = root_push(value);
    let cur_idx = root_push(env);
    loop {
        if env_has_own(root_get(cur_idx), name) {
            env_write(root_get(cur_idx), name, root_get(value_idx));
            roots_truncate(value_idx);
            return;
        }
        match env_parent(root_get(cur_idx)) {
            Some(p) => root_set(cur_idx, p),
            None => {
                env_write(root_get(cur_idx), name, root_get(value_idx));
                roots_truncate(value_idx);
                return;
            }
        }
    }
}
