//! Death pruning for object-ADDRESS-keyed side tables (2026-07-09 GC audit,
//! wave 2 batch B).
//!
//! Around a dozen runtime side tables are keyed by the raw address of an
//! owning heap object (descriptor tables, symbol-keyed properties, closure
//! dynamic props, arguments metadata, recorded prototypes, exotic expandos,
//! array expandos and iterator brands, `node:vm` metadata, fs FileHandle fds).
//! The owning GC types mostly have no
//! finalize hook, so nothing told those tables when the owner died: entries —
//! and any strongly-rooted values inside them (accessor closures, symbol
//! property values, expando values) — leaked forever, and a NEW object
//! allocated at the recycled address inherited the dead owner's entries (the
//! ABA hazard behind e.g. "read-only property" errors on fresh objects).
//!
//! This module supplies the two deadness predicates and the fan-out passes,
//! mirroring the proven Map/Set pattern (`map.rs`:
//! `collect_dead_registered_maps_post_trace` /
//! `is_dead_copied_minor_from_space_map`):
//!
//! * [`prune_dead_owner_side_tables_post_trace`] runs at sweep entry of the
//!   non-copying cycle kinds (marks fresh, nothing freed or reallocated yet)
//!   — wired into `IncrementalSweepState::with_dead_collection_finalize`.
//! * [`prune_dead_owner_side_tables_copied_minor`] runs in the copied-minor
//!   fast path right before the from-space flip — wired into
//!   `finalize_dead_copied_minor_from_space_side_allocations`.
//!
//! Deadness rules (the audit's central caveat): a MINOR trace never marks
//! old-gen/malloc'd objects, so an unmarked header only proves death for an
//! untenured nursery object; everything else is only provably dead after a
//! FULL trace. Both predicates additionally require the owner address to be
//! attributable to THIS thread's heap (arena page classification or the
//! thread's malloc-tracked header list) before reading any header byte —
//! several of the pruned tables are process-global and can hold other
//! threads' heap addresses (whose mark bits this thread's trace says nothing
//! about), plus Box-leaked pseudo-objects (well-known symbols) with no
//! GcHeader at all. Unattributable owners are skipped: the residual leak
//! (entries owned by a foreign thread's dead objects, and owners already
//! freed by an earlier minor malloc sweep) is documented and bounded by
//! cross-thread usage.

use super::*;

/// Post-trace deadness probe. Carries a pass-local, lazily built snapshot of
/// the thread's malloc-tracked headers: the shared
/// `gc_malloc_header_is_tracked` helper force-builds the copied-minor malloc
/// REGISTRY (`ensure_set_built`) — a state transition the fallback
/// mark-sweep path deliberately avoids
/// (`test_copied_minor_malloc_scaling_falls_back_when_registry_unavailable`)
/// — so the probe snapshots `MALLOC_STATE.objects` privately instead. The
/// snapshot is built at most once per pass, and only if some table actually
/// holds a non-arena owner address. No mutator runs between sweep entry and
/// the probes, so it cannot go stale within the pass.
struct PostTraceProbe {
    full_trace: bool,
    malloc_headers: RefCell<Option<std::collections::HashSet<usize>>>,
}

impl PostTraceProbe {
    fn new(full_trace: bool) -> Self {
        Self {
            full_trace,
            malloc_headers: RefCell::new(None),
        }
    }

    fn malloc_header_tracked(&self, header: usize) -> bool {
        let mut slot = self.malloc_headers.borrow_mut();
        let set = slot.get_or_insert_with(|| {
            MALLOC_STATE.with(|s| s.borrow().objects.iter().map(|&h| h as usize).collect())
        });
        set.contains(&header)
    }

    /// True when the side-table owner at `addr` is provably dead at
    /// post-trace time. `expected_obj_type` narrows the check for tables
    /// whose owners are always one GC type (closures, symbols); `None`
    /// accepts any registered GC type. Address reuse note: if the owner died
    /// and its address was already recycled by a LIVE object, this returns
    /// `false` and the (stale) entry survives — same contract as the Map
    /// registry pass; the entry is dropped the first time a post-trace pass
    /// observes the address dead.
    fn owner_is_dead(&self, addr: usize, expected_obj_type: Option<u8>) -> bool {
        let Some((header, in_arena)) = self.attributed_owner_header(addr) else {
            return false;
        };
        if !owner_type_matches(header, expected_obj_type) {
            return false;
        }
        let flags = header.gc_flags;
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED | GC_FLAG_FORWARDED) != 0 {
            return false;
        }
        if self.full_trace {
            return true;
        }
        // Minor trace: unmarked is only meaningful for untenured nursery
        // objects (minors never mark old-gen, and malloc'd objects are
        // black-leafed).
        if !in_arena {
            return false;
        }
        if flags & GC_FLAG_TENURED != 0 {
            return false;
        }
        matches!(
            crate::arena::classify_heap_generation(addr),
            crate::arena::HeapGeneration::Nursery
        )
    }

    /// Attribute `addr` to this thread's heap and return its GcHeader
    /// without ever dereferencing unmapped/foreign memory:
    /// * an address inside this thread's arena page ranges is mapped by
    ///   construction (dealloc'd blocks unregister their ranges first);
    /// * otherwise only membership in this thread's malloc-tracked header
    ///   list proves both ownership and liveness-of-the-mapping (the sweep
    ///   deregisters before dealloc).
    /// Anything else — other threads' heaps, `Box`-leaked pseudo-objects,
    /// handles, stale already-freed malloc addresses — returns `None` (skip).
    fn attributed_owner_header(&self, addr: usize) -> Option<(&'static GcHeader, bool)> {
        if addr < GC_HEADER_SIZE {
            return None;
        }
        let in_arena = !matches!(
            crate::arena::classify_heap_generation(addr),
            crate::arena::HeapGeneration::Unknown
        );
        if in_arena {
            return unsafe {
                crate::value::addr_class::try_read_gc_header(addr).map(|h| (h, true))
            };
        }
        if !crate::value::addr_class::is_plausible_heap_addr(addr) {
            return None;
        }
        let header = addr - GC_HEADER_SIZE;
        if !self.malloc_header_tracked(header) {
            return None;
        }
        Some((unsafe { &*(header as *const GcHeader) }, false))
    }
}

/// Copied-minor from-space deadness: the owner sits in this thread's active
/// from-space (eden or the active survivor half) and was neither marked nor
/// forwarded — every live from-space object was evacuated (FORWARDED) or is
/// pinned-and-marked by this point. Mirrors `is_dead_copied_minor_from_space_map`.
fn owner_is_dead_copied_minor_from_space(addr: usize, expected_obj_type: Option<u8>) -> bool {
    let space = crate::arena::classify_heap_space(addr);
    if !matches!(space, crate::arena::HeapSpace::NurseryEden)
        && space != crate::arena::active_survivor_space()
    {
        return false;
    }
    if addr < GC_HEADER_SIZE {
        return false;
    }
    // The space classification is backed by this thread's live arena page
    // ranges, so the header read is on mapped arena memory.
    let header = unsafe { &*((addr - GC_HEADER_SIZE) as *const GcHeader) };
    if !owner_type_matches(header, expected_obj_type) {
        return false;
    }
    let flags = header.gc_flags;
    flags & GC_FLAG_ARENA != 0 && flags & (GC_FLAG_MARKED | GC_FLAG_FORWARDED) == 0
}

#[inline]
fn owner_type_matches(header: &GcHeader, expected_obj_type: Option<u8>) -> bool {
    match expected_obj_type {
        Some(t) => header.obj_type == t,
        // Reject invalidated (obj_type = 0) and garbage headers: not provably
        // the owner any more, so skip rather than prune.
        None => gc_type_info(header.obj_type).is_some(),
    }
}

/// Post-trace fan-out (full mark-sweep + fallback minor). Runs at sweep
/// entry, before any header is finalized or freed, so deadness probes read
/// intact headers.
pub(super) fn prune_dead_owner_side_tables_post_trace(full_trace: bool) {
    let probe = PostTraceProbe::new(full_trace);
    fan_out(
        &|addr| probe.owner_is_dead(addr, None),
        &|addr| probe.owner_is_dead(addr, Some(GC_TYPE_CLOSURE)),
        &|addr| probe.owner_is_dead(addr, Some(GC_TYPE_STRING)),
    );
    // #6182: drop dead weak-target HOLDERS (WeakRef / FinalizationRegistry /
    // WeakMap-WeakSet entry — all GC_TYPE_OBJECT) from the registry so the
    // copied-minor weak-processing latch (`weak_target_holders_allocated` =
    // registry non-empty) returns to zero once a transient WeakMap and its
    // entries die. The copied-minor fast path prunes inside
    // `process_weak_targets_from_registry`; this covers the full/fallback
    // (non-copying) cycles, which don't run that pass.
    crate::weakref::prune_dead_weak_holders(&|addr| {
        probe.owner_is_dead(addr, Some(GC_TYPE_OBJECT))
    });
}

/// Copied-minor fan-out: prune entries owned by dead from-space objects
/// before the flip destroys their headers. Nursery-only by construction, so
/// the tenured/malloc caveat cannot mis-fire here.
pub(super) fn prune_dead_owner_side_tables_copied_minor() {
    fan_out(
        &|addr| owner_is_dead_copied_minor_from_space(addr, None),
        &|addr| owner_is_dead_copied_minor_from_space(addr, Some(GC_TYPE_CLOSURE)),
        &|addr| owner_is_dead_copied_minor_from_space(addr, Some(GC_TYPE_STRING)),
    );
}

fn fan_out(
    is_dead_owner: &dyn Fn(usize) -> bool,
    is_dead_closure: &dyn Fn(usize) -> bool,
    is_dead_symbol: &dyn Fn(usize) -> bool,
) {
    // Interned key pointers cached in the store-plan cache may die in this
    // collection — flush every cached verdict.
    crate::object::prop_plan::prop_plan_epoch_bump();
    crate::array::prune_dead_array_named_property_owners(is_dead_owner);
    crate::map::prune_dead_map_iterator_array_owners(is_dead_owner);
    crate::set::prune_dead_set_iterator_array_owners(is_dead_owner);
    crate::object::prune_dead_descriptor_owner_entries(is_dead_owner);
    crate::object::prune_dead_arguments_object_entries(is_dead_owner);
    crate::object::prototype_chain::prune_dead_object_prototype_owners(is_dead_owner);
    // #6759 C1: shape records are keyed on keys_array addresses; drop the
    // ones whose keys_array died (memory only — per-hit validation covers
    // correctness for anything this misses).
    crate::object::shapes::prune_dead_shape_keys(is_dead_owner);
    crate::object::exotic_expando::prune_dead_exotic_expando_owners(is_dead_owner);
    crate::symbol::prune_dead_symbol_property_owners(is_dead_owner);
    crate::symbol::prune_dead_symbol_pointers(is_dead_symbol);
    crate::closure::prune_dead_closure_side_table_owners(is_dead_closure);
    crate::node_vm::prune_dead_vm_owner_entries(is_dead_owner);
    crate::fs::prune_dead_filehandle_fd_entries(is_dead_owner);
}
