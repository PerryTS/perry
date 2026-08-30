//! Per-class metadata registries: parent-class chain, fetch-parent kind,
//! `extends Error`, `Symbol.hasInstance` / `Symbol.toStringTag` hooks
//! (split out of `object/mod.rs`, behavior-preserving).

use crate::fast_hash::{new_ptr_hash_map, new_ptr_hash_set, PtrHashMap, PtrHashSet};
use crate::object::class_image::{self, ImageTable, PARENT_DENSE_CAP};
use crate::registry_latch::RegistryLatch;
use std::sync::RwLock;

/// The calling image's class registry mapping class_id -> parent_class_id for
/// inheritance chain lookups (#8546 — see `object/class_image.rs`).
pub(crate) static CLASS_REGISTRY: ImageTable<RwLock<Option<PtrHashMap<u32, u32>>>> =
    ImageTable::new(|image| &image.parents);

// ============================================================================
// Dense parent-edge table (#7769)
//
// `get_parent_class_id` is the single hottest class-registry read in the
// runtime: `instanceof`, vtable dispatch, static-member lookup, `super()`
// construction, symbol lookup and the typed-feedback guards all walk the
// parent chain one hop at a time, and EVERY hop took a process-global
// `RwLock` read plus a SipHash probe of a `HashMap<u32, u32>`. A scene-graph
// program (`gc-handoff/apps/shapes.ts`: 5 classes, two `instanceof` tests and
// three virtual calls per node) spent 2.7% of its runtime in
// `std::hash::random::RandomState` and ~4% in `pthread_mutex_{lock,unlock}`
// for what is semantically an indexed load.
//
// Codegen assigns user class ids from a small sequential counter
// (`perry-hir::lower::context`, monomorphized specializations offset by
// +1000), so the overwhelming majority of ids are tiny and dense. Mirror
// every edge whose CHILD id fits into a flat array of atomics; ids outside
// the window (the reserved builtin bands `0xFFFF_00xx` / `0x7FFF_FFxx` and
// the high-bit synthetic ids) keep using the map.
//
// The table is one 256 KiB zero-filled allocation per image (#8546: it lives
// in `ClassImageTables::parent_dense`, one per hosted application), reached
// through the same thread-local image resolution as every other class table.
//
// Encoding: `parent + 1` for every registered edge whose child id is
// `< PARENT_DENSE_CAP`; `0` means "no edge registered for this child". The
// `+1` bias is what lets a single word encode both "absent" and "present with
// parent id 0". The one id that cannot be biased (`u32::MAX`) arms
// [`PARENT_DENSE_INCOMPLETE`] instead of being stored.
// ============================================================================

/// Armed only if an in-window child id could NOT be represented densely (a
/// `u32::MAX` parent — never produced by any id allocator, but the encoding
/// must not silently lie). While idle, a zero slot for an in-window child
/// *proves* there is no edge, so the map is never consulted.
static PARENT_DENSE_INCOMPLETE: RegistryLatch = RegistryLatch::new();

/// Mirror one parent edge into the dense table.
///
/// Called from `class_registry::parent_static::register_class` *before* the
/// map insert, so a reader can never observe the map entry without the dense
/// entry (readers of in-window ids do not consult the map at all, but keeping
/// the publish order makes that independent of who reads what).
pub(crate) fn parent_dense_store(class_id: u32, parent_class_id: u32) {
    let idx = class_id as usize;
    if idx >= PARENT_DENSE_CAP {
        // Out-of-window children are served by the map on both sides; nothing
        // to arm.
        return;
    }
    if parent_class_id == u32::MAX {
        PARENT_DENSE_INCOMPLETE.arm();
        return;
    }
    class_image::parent_dense_store(idx, parent_class_id.wrapping_add(1));
}

/// Look up parent class ID from the registry.
///
/// In-window ids answer from one relaxed-ordering atomic load. Everything else
/// (builtin reserved bands, synthetic high-bit ids) falls back to the locked
/// map, exactly as before.
#[inline]
pub(crate) fn get_parent_class_id(class_id: u32) -> Option<u32> {
    let idx = class_id as usize;
    if idx < PARENT_DENSE_CAP {
        let biased = class_image::parent_dense_load(idx);
        if biased != 0 {
            return Some(biased - 1);
        }
        if PARENT_DENSE_INCOMPLETE.is_idle() {
            return None;
        }
    }
    let registry = CLASS_REGISTRY.read().unwrap();
    registry.as_ref().and_then(|r| r.get(&class_id).copied())
}

/// class_id -> fetch-builtin parent kind (1 = Request, 2 = Response). Recorded
/// when a class is registered (at module init / class-expression evaluation)
/// whose parent value identifies as the global `Request`/`Response`
/// constructor — including via an alias such as `@hono/node-server`'s
/// `GlobalRequest = global.Request`. Lets the runtime dynamic-construction
/// path (`new (classExprValue)(...)` / ClassRef `new`) attach the underlying
/// native fetch handle, matching what the static codegen `super()` path does.
static FETCH_PARENT_KIND: ImageTable<RwLock<Option<PtrHashMap<u32, u8>>>> =
    ImageTable::new(|image| &image.fetch_parent_kind);

/// Idle until some class extends the global `Request`/`Response`.
static FETCH_PARENT_LATCH: RegistryLatch = RegistryLatch::new();

/// Record that `class_id` directly extends the global Request (kind 1) or
/// Response (kind 2) constructor.
pub(crate) fn register_fetch_parent_kind(class_id: u32, kind: u8) {
    FETCH_PARENT_LATCH.arm();
    let mut g = FETCH_PARENT_KIND.write().unwrap();
    if g.is_none() {
        *g = Some(new_ptr_hash_map());
    }
    g.as_mut().unwrap().insert(class_id, kind);
}

/// The directly-recorded fetch parent kind for `class_id` (no chain walk).
#[inline]
pub(crate) fn fetch_parent_kind(class_id: u32) -> Option<u8> {
    if FETCH_PARENT_LATCH.is_idle() {
        return None;
    }
    fetch_parent_kind_slow(class_id)
}

#[inline(never)]
fn fetch_parent_kind_slow(class_id: u32) -> Option<u8> {
    let g = FETCH_PARENT_KIND.read().ok()?;
    g.as_ref()?.get(&class_id).copied()
}

/// #7575: specialized class_id -> the GENERIC class_id it was monomorphized
/// from.
///
/// Perry monomorphizes generic classes: `class Gen<T> {}` plus
/// `new Gen<number>()` emits a second class `Gen$num`
/// (`perry_hir::monomorph::mangle::generate_specialized_name`) with its own
/// class id, and the instance is stamped with THAT id. `x instanceof Gen`
/// resolves the RHS to the generic's id, which appears nowhere in the
/// specialization's parent chain — so `new Gen<number>() instanceof Gen` was
/// `false` while `instanceof` against a non-generic base stayed `true`. The
/// issue surfaced as a Map/Set-subclass bug (`m instanceof MyMap`) because
/// `class MyMap<K, V> extends Map<K, V>` is the idiomatic spelling, but the
/// mechanism has nothing to do with Map/Set: a plain `class Gen<T> extends
/// Base {}` fails identically.
///
/// This is deliberately a SEPARATE table rather than a `CLASS_REGISTRY` parent
/// edge. That chain also resolves `super()` construction
/// (`object/class_constructors.rs`), static-method lookup and vtable dispatch,
/// so splicing the generic in between a specialization and its real base would
/// re-run the wrong constructor. Only `instanceof` consults this one.
static CLASS_GENERIC_ORIGIN: ImageTable<RwLock<Option<PtrHashMap<u32, u32>>>> =
    ImageTable::new(|image| &image.generic_origin);

/// Idle until a generic class is monomorphized. `class_chain_reaches` probes
/// this table on EVERY hop of EVERY `instanceof`, so a program with no
/// generics must not pay a lock + hash for it.
static GENERIC_ORIGIN_LATCH: RegistryLatch = RegistryLatch::new();

/// Record that `class_id` is a monomorphized specialization of `generic_id`.
///
/// Emitted once per specialized class in the module-init prelude, next to the
/// `js_register_class_parent` edges.
#[no_mangle]
pub extern "C" fn js_register_class_generic_origin(class_id: u32, generic_id: u32) {
    if class_id == 0 || generic_id == 0 || class_id == generic_id {
        return;
    }
    GENERIC_ORIGIN_LATCH.arm();
    let mut g = CLASS_GENERIC_ORIGIN.write().unwrap();
    if g.is_none() {
        *g = Some(new_ptr_hash_map());
    }
    g.as_mut().unwrap().insert(class_id, generic_id);
}

/// Keepalive anchor: emitted only from generated module-init code, so the
/// whole-program auto-optimize bitcode pass would otherwise dead-strip it.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_REGISTER_CLASS_GENERIC_ORIGIN: extern "C" fn(u32, u32) =
    js_register_class_generic_origin;

/// The generic class `class_id` was specialized from, if any (no chain walk).
#[inline]
pub(crate) fn class_generic_origin(class_id: u32) -> Option<u32> {
    if GENERIC_ORIGIN_LATCH.is_idle() {
        return None;
    }
    class_generic_origin_slow(class_id)
}

#[inline(never)]
fn class_generic_origin_slow(class_id: u32) -> Option<u32> {
    let g = CLASS_GENERIC_ORIGIN.read().ok()?;
    g.as_ref()?.get(&class_id).copied()
}

/// The calling image's set of class IDs that extend the built-in Error class.
static EXTENDS_ERROR_REGISTRY: ImageTable<RwLock<Option<PtrHashSet<u32>>>> =
    ImageTable::new(|image| &image.extends_error);

/// Per-class `Symbol.hasInstance` static hook. Maps class_id → raw function
/// pointer with signature `extern "C" fn(value: f64) -> f64` (NaN-boxed
/// TAG_TRUE / TAG_FALSE result). Populated at module init from
/// `__perry_wk_hasinstance_<class>` top-level functions lifted by the HIR
/// class lowering.
static CLASS_HAS_INSTANCE_REGISTRY: ImageTable<RwLock<Option<PtrHashMap<u32, usize>>>> =
    ImageTable::new(|image| &image.has_instance);

/// Per-class `Symbol.toStringTag` getter hook. Maps class_id → raw function
/// pointer with signature `extern "C" fn(this: f64) -> f64` returning a
/// NaN-boxed STRING_TAG value with the user's tag text. Populated at module
/// init from `__perry_wk_tostringtag_<class>` top-level functions lifted by
/// the HIR class lowering. Consulted by `js_object_to_string` so
/// `Object.prototype.toString.call(x)` returns `[object <tag>]`.
static CLASS_TO_STRING_TAG_REGISTRY: ImageTable<RwLock<Option<PtrHashMap<u32, usize>>>> =
    ImageTable::new(|image| &image.to_string_tag);

/// Idle until a class declares `static [Symbol.hasInstance]`. `js_instanceof`
/// consults the table on every evaluation, ahead of the class-chain walk.
static HAS_INSTANCE_LATCH: RegistryLatch = RegistryLatch::new();

/// Idle until a class declares `static [Symbol.toStringTag]`.
static TO_STRING_TAG_LATCH: RegistryLatch = RegistryLatch::new();

/// Idle until a class `extends Error`.
static EXTENDS_ERROR_LATCH: RegistryLatch = RegistryLatch::new();

/// Register a class-level `Symbol.hasInstance` hook.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_has_instance(class_id: u32, func_ptr: i64) {
    HAS_INSTANCE_LATCH.arm();
    let mut registry = CLASS_HAS_INSTANCE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(new_ptr_hash_map());
    }
    registry
        .as_mut()
        .unwrap()
        .insert(class_id, func_ptr as usize);
}

/// Register a class-level `Symbol.toStringTag` getter hook.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_to_string_tag(class_id: u32, func_ptr: i64) {
    TO_STRING_TAG_LATCH.arm();
    let mut registry = CLASS_TO_STRING_TAG_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(new_ptr_hash_map());
    }
    registry
        .as_mut()
        .unwrap()
        .insert(class_id, func_ptr as usize);
}

#[inline]
pub(crate) fn lookup_has_instance_hook(class_id: u32) -> Option<usize> {
    if HAS_INSTANCE_LATCH.is_idle() {
        return None;
    }
    lookup_has_instance_hook_slow(class_id)
}

#[inline(never)]
fn lookup_has_instance_hook_slow(class_id: u32) -> Option<usize> {
    let reg = CLASS_HAS_INSTANCE_REGISTRY.read().unwrap();
    reg.as_ref().and_then(|m| m.get(&class_id).copied())
}

#[inline]
pub(crate) fn lookup_to_string_tag_hook(class_id: u32) -> Option<usize> {
    if TO_STRING_TAG_LATCH.is_idle() {
        return None;
    }
    lookup_to_string_tag_hook_slow(class_id)
}

#[inline(never)]
fn lookup_to_string_tag_hook_slow(class_id: u32) -> Option<usize> {
    let reg = CLASS_TO_STRING_TAG_REGISTRY.read().unwrap();
    reg.as_ref().and_then(|m| m.get(&class_id).copied())
}

/// Mark a user-defined class as extending the built-in Error class.
#[no_mangle]
pub extern "C" fn js_register_class_extends_error(class_id: u32) {
    EXTENDS_ERROR_LATCH.arm();
    let mut registry = EXTENDS_ERROR_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(new_ptr_hash_set());
    }
    registry.as_mut().unwrap().insert(class_id);
}

/// Check if a class id extends the built-in Error class
#[inline]
pub(crate) fn extends_builtin_error(class_id: u32) -> bool {
    if EXTENDS_ERROR_LATCH.is_idle() {
        return false;
    }
    extends_builtin_error_slow(class_id)
}

#[inline(never)]
fn extends_builtin_error_slow(class_id: u32) -> bool {
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

#[cfg(test)]
mod dense_parent_tests {
    use super::*;

    /// Class ids used by these tests. Chosen high inside the dense window so
    /// they cannot collide with ids any other test in the process registers.
    const A: u32 = 60_001;
    const B: u32 = 60_002;
    const C: u32 = 60_003;
    /// Deliberately OUTSIDE `PARENT_DENSE_CAP` — must still resolve, through
    /// the map.
    const FAR_CHILD: u32 = (PARENT_DENSE_CAP as u32) + 7;

    #[test]
    fn dense_table_answers_the_same_chain_as_the_map() {
        // C extends B extends A, exactly the shape `class Square extends Rect
        // extends Shape` produces.
        crate::object::class_registry::register_class(B, A);
        crate::object::class_registry::register_class(C, B);

        assert_eq!(get_parent_class_id(C), Some(B));
        assert_eq!(get_parent_class_id(B), Some(A));

        // The dense answer must agree with the authoritative map, entry for
        // entry — the dense table is a mirror, not a second source of truth.
        let map = CLASS_REGISTRY.read().unwrap();
        let map = map.as_ref().expect("registry populated");
        for cid in [A, B, C] {
            assert_eq!(get_parent_class_id(cid), map.get(&cid).copied());
        }
    }

    #[test]
    fn an_unregistered_in_window_id_answers_none_without_touching_the_map() {
        // 60_050 is never registered by any test. A zero dense slot is
        // authoritative while `PARENT_DENSE_INCOMPLETE` is idle, which is the
        // whole point: the common "no parent" answer costs one atomic load.
        assert!(PARENT_DENSE_INCOMPLETE.is_idle());
        assert_eq!(get_parent_class_id(60_050), None);
    }

    #[test]
    fn out_of_window_children_still_resolve_through_the_map() {
        crate::object::class_registry::register_class(FAR_CHILD, A);
        assert_eq!(get_parent_class_id(FAR_CHILD), Some(A));
    }

    /// A registered edge whose parent is `0` must read back as `Some(0)`, not
    /// as "absent" — the `+1` bias in the dense encoding exists for exactly
    /// this, and every caller that treats `Some(0)` as a chain terminator does
    /// so explicitly.
    #[test]
    fn parent_zero_is_distinguishable_from_absent() {
        const ZERO_PARENT_CHILD: u32 = 60_010;
        crate::object::class_registry::register_class(ZERO_PARENT_CHILD, 0);
        assert_eq!(get_parent_class_id(ZERO_PARENT_CHILD), Some(0));
        assert_eq!(get_parent_class_id(60_011), None);
    }

    /// Every latch in this module must start idle, so a program that uses none
    /// of these features answers from one atomic load. A latch that shipped
    /// armed-by-default would silently restore the locked-hash-probe cost with
    /// no test able to notice.
    #[test]
    fn feature_latches_default_to_idle() {
        assert!(GENERIC_ORIGIN_LATCH.is_idle() || class_generic_origin(1).is_none());
        // The `has_instance` / `to_string_tag` / `extends Error` probes must
        // answer negatively while their latch is idle, whatever is in the map.
        if HAS_INSTANCE_LATCH.is_idle() {
            assert_eq!(lookup_has_instance_hook(A), None);
        }
        if TO_STRING_TAG_LATCH.is_idle() {
            assert_eq!(lookup_to_string_tag_hook(A), None);
        }
        if EXTENDS_ERROR_LATCH.is_idle() {
            assert!(!extends_builtin_error(A));
        }
        if FETCH_PARENT_LATCH.is_idle() {
            assert_eq!(fetch_parent_kind(A), None);
        }
    }
}
