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

/// Upper bound on distinct interned env keys. Real bodies reuse a tiny
/// identifier vocabulary; this only guards against codegen-heavy / adversarial
/// `new Function` bodies with an unbounded set of distinct local names, each of
/// which would otherwise pin one never-freed longlived allocation for the
/// thread's life.
const ENV_KEY_CACHE_MAX: usize = 4096;

fn key_string(name: &str) -> *const crate::string::StringHeader {
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

/// #6693 surgical prototype (gated by `PERRY_DYN_FAST_SCOPE`): a lean own-field
/// probe for scope objects. Scopes are known-simple — null-proto,
/// `GC_TYPE_OBJECT`, string keys, no accessors — so the general
/// `js_object_get_field_by_name` slow path (proxy/handle/prototype/descriptor
/// vets + key hashing + full keys scan) is pure overhead on the interpreter's
/// hottest operation. This reuses the tested read-plan cache: after the first
/// probe of a `(keys_array, key)` pair every later read is an O(1) index into
/// the field slot (and same-shape sibling scopes share one keys_array via the
/// transition cache, so the cache carries across calls). Like the codegen fast
/// lane it accelerates HITS only and defers anything it can't prove to the
/// authoritative slow path — a capped scan is never mistaken for absence.
enum ScopeProbe {
    /// Own binding found; carries its value bits.
    Hit(f64),
    /// Own binding provably absent (exhaustive scan of a dense keys array on a
    /// null-proto object): the caller may walk to the parent with no slow vet.
    Absent,
    /// Undecided (no keys array / a truncated scan / an overflow slot): the
    /// caller must fall back to the authoritative slow read.
    Bail,
}

/// Probe a single scope for `key` without any allocation (so the raw object
/// pointer stays valid for the whole call — no rooting needed inside).
fn scope_probe(env: f64, key: *const crate::string::StringHeader) -> ScopeProbe {
    let o = crate::value::js_nanbox_get_pointer(env) as *const crate::object::ObjectHeader;
    if o.is_null() {
        return ScopeProbe::Bail;
    }
    unsafe {
        let keys = (*o).keys_array;
        if keys.is_null() {
            return ScopeProbe::Bail;
        }
        let alloc_limit = std::cmp::max((*o).field_count, 8);
        if let Some(idx) = crate::object::prop_plan::read_plan_lookup(keys as usize, key as usize) {
            if idx < alloc_limit {
                let v = crate::object::js_object_get_field(o, idx);
                return ScopeProbe::Hit(f64::from_bits(v.bits()));
            }
            return ScopeProbe::Bail;
        }
        let full = crate::array::js_array_length(keys) as usize;
        let n = crate::array::keys_array_len_capped_to_capacity(keys);
        for i in 0..n as u32 {
            let kv = crate::array::js_array_get(keys, i);
            if crate::string::js_string_key_matches(kv, key) {
                crate::object::prop_plan::read_plan_record(keys as usize, key as usize, i);
                if i < alloc_limit {
                    let v = crate::object::js_object_get_field(o, i);
                    return ScopeProbe::Hit(f64::from_bits(v.bits()));
                }
                return ScopeProbe::Bail;
            }
        }
        if n == full {
            ScopeProbe::Absent
        } else {
            ScopeProbe::Bail
        }
    }
}

/// Read `name`, walking the scope chain. `None` when no scope binds it (the
/// caller then falls back to the real `globalThis`).
///
/// The cursor lives in a rooted slot: `env_has_own` / `env_parent` allocate
/// key strings, and a moving collection triggered by those allocations would
/// otherwise leave a raw `f64` cursor stale.
///
/// #6693 hot path: this runs on EVERY identifier reference. With the fast
/// scope accessor it resolves a hit via the read-plan cache; otherwise it reads
/// the field FIRST and only falls back to `env_has_own` when the read yields
/// `undefined` (a null-proto scope reads a missing key as exactly `undefined`,
/// so the common non-`undefined` binding costs a SINGLE field-op, not the old
/// `has_own` + `read` pair — the field-op, not the key allocation, dominates).
pub(crate) fn lookup(env: f64, name: &str) -> Option<f64> {
    let fast = super::fast_scope_enabled();
    let cur_idx = root_push(env);
    let key = if fast { key_string(name) } else { std::ptr::null() };
    loop {
        if fast {
            match scope_probe(root_get(cur_idx), key) {
                ScopeProbe::Hit(v) => {
                    roots_truncate(cur_idx);
                    return Some(v);
                }
                ScopeProbe::Absent => match env_parent(root_get(cur_idx)) {
                    Some(p) => {
                        root_set(cur_idx, p);
                        continue;
                    }
                    None => {
                        roots_truncate(cur_idx);
                        return None;
                    }
                },
                ScopeProbe::Bail => {}
            }
        }
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
    let fast = super::fast_scope_enabled();
    let cur_idx = root_push(env);
    let key = if fast { key_string(name) } else { std::ptr::null() };
    loop {
        let present = if fast {
            match scope_probe(root_get(cur_idx), key) {
                ScopeProbe::Hit(_) => Some(true),
                ScopeProbe::Absent => Some(false),
                ScopeProbe::Bail => None,
            }
        } else {
            None
        };
        let present = present.unwrap_or_else(|| env_has_own(root_get(cur_idx), name));
        if present {
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
    let fast = super::fast_scope_enabled();
    let value_idx = root_push(value);
    let cur_idx = root_push(env);
    let key = if fast { key_string(name) } else { std::ptr::null() };
    loop {
        let present = if fast {
            match scope_probe(root_get(cur_idx), key) {
                ScopeProbe::Hit(_) => Some(true),
                ScopeProbe::Absent => Some(false),
                ScopeProbe::Bail => None,
            }
        } else {
            None
        };
        let present = present.unwrap_or_else(|| env_has_own(root_get(cur_idx), name));
        if present {
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
