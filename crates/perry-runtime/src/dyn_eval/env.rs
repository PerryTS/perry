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
/// Optional ObjectEnvironmentRecord bindings for this scope. The space keeps
/// the slot unreachable through interpreted identifiers, like `PARENT_KEY`.
const OBJECT_BINDINGS_KEY: &str = "perry dyn object bindings";

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

/// Upper bound on distinct interned env keys. Real bodies reuse a tiny
/// identifier vocabulary; this only guards against codegen-heavy / adversarial
/// `new Function` bodies with an unbounded set of distinct local names, each of
/// which would otherwise pin one never-freed longlived allocation for the
/// thread's life.
const ENV_KEY_CACHE_MAX: usize = 4096;

pub(super) fn key_string(name: &str) -> *const crate::string::StringHeader {
    if let Some(ptr) = ENV_KEY_CACHE.with(|c| c.borrow().get(name).copied()) {
        return ptr;
    }
    // Past the cap, fall back to the pre-cache path: a fresh GC-managed key per
    // access (correct — this is exactly the old behavior — just uncached and
    // collectable, so the longlived arena can't grow without limit).
    if ENV_KEY_CACHE.with(|c| c.borrow().len()) >= ENV_KEY_CACHE_MAX {
        return crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
            as *const crate::string::StringHeader;
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

/// Allocate an object-environment scope. Identifier reads and writes delegate
/// to `bindings` while lexical declarations continue to live on the wrapper.
/// This is the shared seam used by VM globals and compileFunction context
/// extensions; the bindings object remains live and is never copied/mutated
/// with interpreter bookkeeping.
pub(crate) fn env_new_object(parent: Option<f64>, bindings: f64) -> f64 {
    let bindings_idx = root_push(bindings);
    let env = match parent {
        Some(parent) => env_new(parent),
        None => env_new_root(),
    };
    let env_idx = root_push(env);
    let key = key_string(OBJECT_BINDINGS_KEY);
    crate::object::js_object_set_field_by_name(
        env_object_ptr(root_get(env_idx)),
        key,
        root_get(bindings_idx),
    );
    let env = root_get(env_idx);
    roots_truncate(bindings_idx);
    env
}

fn env_object_bindings(env: f64) -> Option<f64> {
    let value = env_read(env, OBJECT_BINDINGS_KEY);
    (!crate::value::JSValue::from_bits(value.to_bits()).is_undefined()).then_some(value)
}

fn object_has_binding(bindings: f64, name: &str) -> bool {
    let key = crate::value::js_nanbox_string(key_string(name) as i64);
    crate::value::js_is_truthy(crate::object::js_object_has_property(bindings, key)) != 0
}

fn object_read_binding(bindings: f64, name: &str) -> f64 {
    let key = crate::value::js_nanbox_string(key_string(name) as i64);
    crate::proxy::js_reflect_get(bindings, key, bindings)
}

fn object_write_binding(bindings: f64, name: &str, value: f64, strict: bool) {
    let key = crate::value::js_nanbox_string(key_string(name) as i64);
    crate::proxy::js_put_value_set(bindings, key, value, bindings, strict as i32);
}

/// Scope objects are private null-prototype data objects. Their immutable
/// key list resolves a name to a slot, including overflow slots; no generic
/// property machinery, getters, or Function.prototype checks are needed.
fn own_slot(env: f64, name: &str) -> Option<u32> {
    let obj = env_object_ptr(env);
    unsafe {
        let keys = crate::object::object_keys(obj);
        crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), name.as_bytes())
    }
}

fn own_value(env: f64, name: &str) -> Option<f64> {
    own_slot(env, name).map(|slot| {
        f64::from_bits(crate::object::js_object_get_field(env_object_ptr(env), slot).bits())
    })
}

fn env_parent(env: f64) -> Option<f64> {
    own_value(env, PARENT_KEY)
}

fn env_has_own(env: f64, name: &str) -> bool {
    own_slot(env, name).is_some()
}

pub(crate) fn variable_environment(env: f64) -> f64 {
    let cur_idx = root_push(env);
    loop {
        if env_object_bindings(root_get(cur_idx)).is_some() {
            let result = root_get(cur_idx);
            roots_truncate(cur_idx);
            return result;
        }
        match env_parent(root_get(cur_idx)) {
            Some(parent) => root_set(cur_idx, parent),
            None => {
                let result = root_get(cur_idx);
                roots_truncate(cur_idx);
                return result;
            }
        }
    }
}

pub(crate) fn ensure_var_binding(env: f64, name: &str) {
    let env_idx = root_push(env);
    if let Some(bindings) = env_object_bindings(root_get(env_idx)) {
        if !object_has_binding(bindings, name) {
            object_write_binding(
                env_object_bindings(root_get(env_idx)).unwrap(),
                name,
                super::bridge::undefined(),
                false,
            );
        }
    } else if !env_has_own(root_get(env_idx), name) {
        define(root_get(env_idx), name, super::bridge::undefined());
    }
    roots_truncate(env_idx);
}

fn env_read(env: f64, name: &str) -> f64 {
    own_value(env, name).unwrap_or_else(super::bridge::undefined)
}

fn env_write(env: f64, name: &str, value: f64) {
    let env_idx = root_push(env);
    let value_idx = root_push(value);
    if let Some(slot) = own_slot(root_get(env_idx), name) {
        let obj = env_object_ptr(root_get(env_idx));
        let value = crate::value::JSValue::from_bits(root_get(value_idx).to_bits());
        if slot < unsafe { crate::object::object_live_slot_count(obj) } {
            crate::object::js_object_set_field(obj, slot, value);
        } else {
            crate::object::overflow_set(obj as usize, slot as usize, value.bits());
        }
        roots_truncate(env_idx);
        return;
    }
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
/// Keep the cursor rooted while object-environment hooks can run user code.
/// Private lexical scopes resolve reads and writes directly through slots.
pub(crate) fn lookup(env: f64, name: &str) -> Option<f64> {
    let cur_idx = root_push(env);
    loop {
        if let Some(value) = own_value(root_get(cur_idx), name) {
            roots_truncate(cur_idx);
            return Some(value);
        }
        let has_object_binding = env_object_bindings(root_get(cur_idx))
            .is_some_and(|bindings| object_has_binding(bindings, name));
        if has_object_binding {
            let value = object_read_binding(env_object_bindings(root_get(cur_idx)).unwrap(), name);
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
        let present = env_has_own(root_get(cur_idx), name);
        if present {
            roots_truncate(cur_idx);
            return true;
        }
        if let Some(bindings) = env_object_bindings(root_get(cur_idx)) {
            if object_has_binding(bindings, name) {
                roots_truncate(cur_idx);
                return true;
            }
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
pub(crate) fn assign(env: f64, name: &str, value: f64, strict: bool) {
    let value_idx = root_push(value);
    let cur_idx = root_push(env);
    loop {
        let present = env_has_own(root_get(cur_idx), name);
        if present {
            env_write(root_get(cur_idx), name, root_get(value_idx));
            roots_truncate(value_idx);
            return;
        }
        let has_object_binding = env_object_bindings(root_get(cur_idx))
            .is_some_and(|bindings| object_has_binding(bindings, name));
        if has_object_binding {
            object_write_binding(
                env_object_bindings(root_get(cur_idx)).unwrap(),
                name,
                root_get(value_idx),
                strict,
            );
            roots_truncate(value_idx);
            return;
        }
        match env_parent(root_get(cur_idx)) {
            Some(p) => root_set(cur_idx, p),
            None => {
                if strict {
                    roots_truncate(value_idx);
                    super::bridge::throw_reference_error(&format!("{name} is not defined"));
                }
                if let Some(bindings) = env_object_bindings(root_get(cur_idx)) {
                    object_write_binding(bindings, name, root_get(value_idx), strict);
                } else {
                    env_write(root_get(cur_idx), name, root_get(value_idx));
                }
                roots_truncate(value_idx);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_scope_slots_keep_undefined_shadowing_overflow_and_assignment() {
        let roots = root_push(super::super::bridge::undefined());
        let outer = root_push(env_new_root());
        define(root_get(outer), "shadow", 42.0);
        define(root_get(outer), "counter", 1.0);
        let inner = root_push(env_new(root_get(outer)));
        define(root_get(inner), "shadow", super::super::bridge::undefined());
        for i in 0..64 {
            define(root_get(inner), &format!("local{i}"), i as f64);
        }
        assert_eq!(
            lookup(root_get(inner), "shadow").unwrap().to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(lookup(root_get(inner), "local63"), Some(63.0));
        assert_eq!(lookup(root_get(inner), "local64"), None);
        assign(root_get(inner), "counter", 2.0, false);
        assign(root_get(inner), "local63", 99.0, false);
        assert_eq!(lookup(root_get(outer), "counter"), Some(2.0));
        assert_eq!(lookup(root_get(inner), "local63"), Some(99.0));
        assert!(is_bound(root_get(inner), "shadow"));
        // A sibling's shorter key list can share a backing with this scope.
        // A binding from the long list must stay absent on the short one.
        let short = root_push(env_new(root_get(outer)));
        define(root_get(short), "shadow", 7.0);
        assert_eq!(lookup(root_get(short), "local63"), None);
        assert_eq!(lookup(root_get(short), "shadow"), Some(7.0));
        roots_truncate(roots);
    }
}
