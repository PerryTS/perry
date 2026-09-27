//! Per-class metadata registries: parent-class chain, fetch-parent kind,
//! `extends Error`, `Symbol.hasInstance` / `Symbol.toStringTag` hooks
//! (split out of `object/mod.rs`, behavior-preserving).

use crate::fast_hash::{new_ptr_hash_map, new_ptr_hash_set, PtrHashMap, PtrHashSet};
use crate::object::class_image::{self, ImageTable, PARENT_DENSE_CAP};
use crate::registry_latch::RegistryLatch;
use std::sync::RwLock;

/// The calling image's class_id -> parent_class_id map for the children the
/// dense table below cannot hold (#8546 — see `object/class_image.rs`). Not a
/// complete view: an in-window child's edge lives ONLY in the dense table
/// (#11502), so the map is private and every reader goes through
/// [`get_parent_class_id`] / [`any_registered_ancestor`].
static CLASS_REGISTRY: ImageTable<RwLock<Option<PtrHashMap<u32, u32>>>> =
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
// +1000), so the overwhelming majority of ids are tiny and dense. Every edge
// whose CHILD id fits is stored in a flat array of atomics, and ONLY there
// (#11502: it used to be inserted into the map as well, under the map's write
// lock, for no reader); ids outside the window (the reserved builtin bands
// `0xFFFF_00xx` / `0x7FFF_FFxx` and the high-bit synthetic ids) keep using
// the map.
//
// The table is one 256 KiB zeroed allocation per image, created on its first
// representable edge (#8546: `ClassImageTables::parent_dense`, one per hosted
// application); only the pages holding registered children become resident.
// Reads before registration answer absent without an allocation. Initialized
// reads retain the atomic indexed load, reached through the same thread-local
// image resolution as every other class table.
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

/// Publish one parent edge: into the dense table when the child is in the
/// window and the parent is representable, otherwise into the map. Exactly one
/// of the two receives it, which is sound because no reader consults the map
/// for an in-window child while the dense slot is set or
/// [`PARENT_DENSE_INCOMPLETE`] is idle — see [`get_parent_class_id`].
pub(crate) fn publish_parent_edge(class_id: u32, parent_class_id: u32) {
    if parent_dense_store(class_id, parent_class_id) {
        return;
    }
    let mut registry = CLASS_REGISTRY.write().unwrap();
    registry
        .get_or_insert_with(new_ptr_hash_map)
        .insert(class_id, parent_class_id);
}

/// Store one parent edge into the dense table; `false` when it cannot be held
/// there and must go to the map instead.
fn parent_dense_store(class_id: u32, parent_class_id: u32) -> bool {
    let idx = class_id as usize;
    if idx >= PARENT_DENSE_CAP {
        // Out-of-window children are served by the map on both sides; nothing
        // to arm.
        return false;
    }
    if parent_class_id == u32::MAX {
        // Arm BEFORE the caller's map insert, so a zero dense slot stops
        // proving absence no later than the map starts holding the edge.
        PARENT_DENSE_INCOMPLETE.arm();
        return false;
    }
    class_image::parent_dense_store(idx, parent_class_id.wrapping_add(1));
    true
}

/// Look up parent class ID from the registry.
///
/// In-window ids answer from an acquire atomic load once initialized. Everything else
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

/// Walk `class_id`'s registered ancestors (not `class_id` itself) for at most
/// `max_hops` hops, and report whether `hit` accepts any of them. A missing
/// edge or a `0` parent ends the chain.
///
/// The one chain walk for code that tests membership of an ancestor in some
/// class-id set. It goes through [`get_parent_class_id`] rather than the map,
/// which since #11502 does not hold in-window edges at all.
pub(crate) fn any_registered_ancestor(
    class_id: u32,
    max_hops: usize,
    mut hit: impl FnMut(u32) -> bool,
) -> bool {
    let mut current = class_id;
    for _ in 0..max_hops {
        match get_parent_class_id(current) {
            Some(parent) if parent != 0 => {
                if hit(parent) {
                    return true;
                }
                current = parent;
            }
            _ => break,
        }
    }
    false
}

/// An upper bound on the number of parent edges the calling image holds: one
/// per dense slot plus one per map entry. A chain walk that takes more hops
/// than this has revisited a child, i.e. the registry holds a cycle.
pub(crate) fn parent_edge_count_bound() -> usize {
    let registry = CLASS_REGISTRY.read().unwrap();
    PARENT_DENSE_CAP + registry.as_ref().map_or(0, |r| r.len())
}

/// `(entries, bytes)` of the out-of-window parent map, for the side-table
/// census; `None` until the first out-of-window edge.
pub(crate) fn parent_map_census() -> Option<(usize, usize)> {
    let registry = CLASS_REGISTRY.read().ok()?;
    let map = registry.as_ref()?;
    Some((map.len(), crate::gc::census::map_bytes(map)))
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
    {
        let mut g = CLASS_GENERIC_ORIGIN.write().unwrap();
        if g.is_none() {
            *g = Some(new_ptr_hash_map());
        }
        g.as_mut().unwrap().insert(class_id, generic_id);
    }
    // Arming the latch and adding the edge redirect BOTH prototype-object
    // readers (`class_prototype_object`, `class_decl_prototype_object`) and
    // `lookup_prototype_method`'s chain hop to the generic's id, so a cached
    // per-class-id chain verdict must retire (#10696).
    crate::object::class_lookup_surface_gen_bump();
}

/// Keepalive anchor: emitted only from generated module-init code, so the
/// whole-program auto-optimize bitcode pass would otherwise dead-strip it.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
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
    let Some(reg) = registry.as_ref() else {
        return false;
    };
    reg.contains(&class_id) || any_registered_ancestor(class_id, 32, |p| reg.contains(&p))
}

/// Resolve the Error-family prototype at the bottom of a registered class
/// chain. Callers first establish [`extends_builtin_error`]; returning
/// `"Error"` on an incomplete/cyclic chain is the same fallback used by
/// ordinary prototype-property lookup.
pub(crate) fn builtin_error_prototype_name(class_id: u32) -> &'static str {
    let mut current = class_id;
    for _ in 0..32 {
        match current {
            crate::error::CLASS_ID_TYPE_ERROR => return "TypeError",
            crate::error::CLASS_ID_RANGE_ERROR => return "RangeError",
            crate::error::CLASS_ID_REFERENCE_ERROR => return "ReferenceError",
            crate::error::CLASS_ID_SYNTAX_ERROR => return "SyntaxError",
            crate::error::CLASS_ID_EVAL_ERROR => return "EvalError",
            crate::error::CLASS_ID_URI_ERROR => return "URIError",
            crate::error::CLASS_ID_AGGREGATE_ERROR => return "AggregateError",
            crate::error::CLASS_ID_ERROR => return "Error",
            _ => match get_parent_class_id(current) {
                Some(parent) if parent != 0 && parent != current => current = parent,
                _ => break,
            },
        }
    }
    "Error"
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

    /// The map entry for `class_id`, bypassing the dense table.
    fn map_entry(class_id: u32) -> Option<u32> {
        let map = CLASS_REGISTRY.read().unwrap();
        map.as_ref().and_then(|m| m.get(&class_id).copied())
    }

    #[test]
    fn in_window_edges_are_held_only_by_the_dense_table() {
        // C extends B extends A, exactly the shape `class Square extends Rect
        // extends Shape` produces.
        crate::object::class_registry::register_class(B, A);
        crate::object::class_registry::register_class(C, B);

        assert_eq!(get_parent_class_id(C), Some(B));
        assert_eq!(get_parent_class_id(B), Some(A));
        assert_eq!(get_parent_class_id(A), None);

        // #11502: the dense table is the edge's only copy. Publishing it into
        // the map as well took the map's write lock and grew the map on every
        // first registration, for no reader.
        for cid in [B, C] {
            assert_eq!(
                map_entry(cid),
                None,
                "in-window edge {cid} was double-published into the map"
            );
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
        assert_eq!(map_entry(FAR_CHILD), Some(A), "the map is its only home");
    }

    /// #11502: the `extends Error` / `DataView` / typed-array probes used to
    /// walk the map directly. Once in-window edges live only in the dense
    /// table, such a walk sees no ancestors at all, so a grandchild of a
    /// registered builtin subclass stops counting as one.
    #[test]
    fn builtin_subclass_probes_walk_edges_held_only_densely() {
        // Registering bumps the process-global store-plan epoch that
        // `re_registering_the_same_edge_flushes_nothing` asserts on.
        let _lock = crate::gc::global_side_table_test_lock();
        const ERR_BASE: u32 = 60_101;
        const VIEW_BASE: u32 = 60_111;
        const TYPED_BASE: u32 = 60_121;
        const UNRELATED: u32 = 60_131;
        let register = crate::object::class_registry::register_class;

        js_register_class_extends_error(ERR_BASE);
        register(60_102, ERR_BASE);
        register(60_103, 60_102);
        assert!(extends_builtin_error(60_103));
        assert!(!extends_builtin_error(UNRELATED));

        crate::object::data_view_registry::js_register_class_extends_data_view(VIEW_BASE);
        register(60_112, VIEW_BASE);
        register(60_113, 60_112);
        assert!(crate::object::extends_builtin_data_view(60_113));
        assert!(!crate::object::extends_builtin_data_view(UNRELATED));

        // The typed-array probe follows arbitrarily deep hierarchies (its hop
        // bound is the registry's edge count, not 32), so chain 40 levels.
        crate::object::data_view_registry::js_register_class_extends_typed_array(TYPED_BASE);
        let mut parent = TYPED_BASE;
        for child in 60_140..60_180 {
            register(child, parent);
            parent = child;
        }
        assert!(crate::object::extends_builtin_typed_array(parent));
        assert!(!crate::object::extends_builtin_typed_array(UNRELATED));
    }

    /// The typed-array probe's hop bound must still terminate on a malformed
    /// (cyclic) chain now that the edges are not all in the map it used to
    /// count.
    #[test]
    fn unbounded_ancestor_walk_terminates_on_a_cycle() {
        let _lock = crate::gc::global_side_table_test_lock();
        const X: u32 = 60_191;
        const Y: u32 = 60_192;
        crate::object::class_registry::register_class(X, Y);
        crate::object::class_registry::register_class(Y, X);
        let bound = parent_edge_count_bound();
        assert!(
            bound >= PARENT_DENSE_CAP,
            "the bound must count dense edges"
        );
        let mut hops = 0usize;
        assert!(!any_registered_ancestor(X, bound, |_| {
            hops += 1;
            false
        }));
        assert_eq!(hops, bound, "the walk must stop at the bound, not before");
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

    /// Re-registering an edge that is already published must be a no-op.
    ///
    /// Every allocation of an inheriting class calls `register_class`
    /// (`object_alloc_class_inline_keys_impl`), so a bump here is a bump per
    /// `new`, and `prop_plan_epoch_bump`'s own contract says its callers are
    /// "rare, cold paths by construction" — an epoch bump throws away every
    /// cached store plan in the program.
    #[test]
    fn re_registering_the_same_edge_flushes_nothing() {
        // The epoch this asserts on is PROCESS-global, so any other test
        // thread installing a descriptor or registering a class inside the
        // window below moves it and fails this test for reasons that have
        // nothing to do with re-registration. Serialize against the tests
        // that do (they all take this lock).
        let _lock = crate::gc::global_side_table_test_lock();
        const CHILD: u32 = 60_020;
        const PARENT: u32 = 60_021;
        crate::object::class_registry::register_class(CHILD, PARENT);

        let epoch_after_first = crate::object::prop_plan::prop_plan_semantic_epoch();
        for _ in 0..8 {
            crate::object::class_registry::register_class(CHILD, PARENT);
        }
        assert_eq!(
            crate::object::prop_plan::prop_plan_semantic_epoch(),
            epoch_after_first,
            "re-registering an unchanged edge must not invalidate cached store plans"
        );
        assert_eq!(get_parent_class_id(CHILD), Some(PARENT));
    }

    /// The other direction, which is what keeps the skip honest: a CHANGED
    /// parent is a different chain, so it must publish and flush.
    #[test]
    fn re_parenting_still_publishes_and_flushes() {
        const CHILD: u32 = 60_030;
        const FIRST: u32 = 60_031;
        const SECOND: u32 = 60_032;
        crate::object::class_registry::register_class(CHILD, FIRST);
        let before = crate::object::prop_plan::prop_plan_semantic_epoch();

        crate::object::class_registry::register_class(CHILD, SECOND);

        assert_ne!(
            crate::object::prop_plan::prop_plan_semantic_epoch(),
            before,
            "a re-parent changes what the chain intercepts and must flush plans"
        );
        assert_eq!(get_parent_class_id(CHILD), Some(SECOND));
        assert_eq!(map_entry(CHILD), None, "re-parenting double-published");
    }
}
