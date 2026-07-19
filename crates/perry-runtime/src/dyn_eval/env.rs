//! Interpreter environments (#6559).
//!
//! A scope is an ordinary runtime object (null prototype, class_id 0):
//! variable name → value as properties, parent scope under a key that can
//! never collide with a JS identifier. Environments therefore live in the
//! normal GC object graph — an interpreted closure keeps its whole defining
//! chain alive through one traced capture slot, and moving collections
//! relocate scopes like any other object (no Rust-side pointer can go stale
//! because every held value routes through the rooted stack in `mod.rs`).

use std::cell::RefCell;
use std::collections::HashMap;

use super::{root_get, root_push, root_set, roots_truncate};

/// Parent-scope key. Contains a space, so no declared identifier can ever
/// shadow or collide with it (interpreted code only reaches environments via
/// identifier resolution, never via computed access).
const PARENT_KEY: &str = "perry dyn parent";

thread_local! {
    /// identifier name → its cached `StringHeader`. Every scope-chain read /
    /// write allocated a fresh heap `StringHeader` for the key on the old
    /// path (`js_string_from_bytes` never SSO-inlines), so a hot validator
    /// that touches `value` / `ok` / a loop var thousands of times burned a
    /// heap allocation per access — a top #6693 execution cost. Env keys are
    /// the same small vocabulary reused forever, so we allocate each once in
    /// the LONGLIVED arena (stable pointer for the thread's life, never
    /// swept/moved — issue #179, the `PARSE_KEY_CACHE` precedent) and reuse
    /// the pointer. Rooted by `scan_env_key_cache_mut` (called from
    /// `scan_dyn_eval_roots_mut`).
    static ENV_KEY_CACHE: RefCell<HashMap<Box<str>, *const crate::string::StringHeader>> =
        RefCell::new(HashMap::new());
}

fn key_string(name: &str) -> *const crate::string::StringHeader {
    if let Some(ptr) = ENV_KEY_CACHE.with(|c| c.borrow().get(name).copied()) {
        return ptr;
    }
    let ptr = crate::string::js_string_from_bytes_longlived(name.as_ptr(), name.len() as u32);
    ENV_KEY_CACHE.with(|c| {
        c.borrow_mut().insert(name.into(), ptr);
    });
    ptr
}

/// Mark the cached longlived key strings so a collection never treats them as
/// garbage (belt-and-suspenders — longlived blocks are never reset — and it
/// rewrites the slot on the rare evacuating pass, matching `PARSE_KEY_CACHE`).
pub(super) fn scan_env_key_cache_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ENV_KEY_CACHE.with(|c| {
        for ptr in c.borrow_mut().values_mut() {
            visitor.visit_tagged_raw_const_ptr_slot(ptr, crate::value::STRING_TAG);
        }
    });
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
///
/// #6693 hot path: this runs on EVERY identifier reference, so it reads the
/// field FIRST and only falls back to `env_has_own` when the read yields
/// `undefined`. Because scopes are null-proto objects, a missing key reads as
/// exactly `undefined`, and the overwhelmingly common binding (a parameter /
/// `let` / loop var holding a non-`undefined` value) is then resolved in a
/// SINGLE object field-op instead of the previous `has_own` + `read` pair —
/// the field-op (key hash + keys-array probe), not the key allocation, is the
/// dominant per-lookup cost. The `has_own` disambiguation only runs for the
/// rare binding whose value genuinely is `undefined`.
pub(crate) fn lookup(env: f64, name: &str) -> Option<f64> {
    let cur_idx = root_push(env);
    loop {
        let value = env_read(root_get(cur_idx), name);
        if value.to_bits() != crate::value::TAG_UNDEFINED {
            roots_truncate(cur_idx);
            return Some(value);
        }
        // Read was `undefined`: either this scope binds it to `undefined`, or
        // the key is absent and we must keep walking. Disambiguate with the
        // presence check (only reached in the uncommon undefined-value case).
        if env_has_own(root_get(cur_idx), name) {
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
