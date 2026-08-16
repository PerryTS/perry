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
//! ★ #8174 — WHY THE LIST IS A REGISTRY NOW. For a table that is REKEYED
//! rather than re-derived (its key slot is rewritten in place by
//! `RuntimeRootVisitor::visit_metadata_*`), a missing prune is not a leak. The
//! arena recycles the dead key's address, the recycled bytes are read as a
//! `GcHeader`, and a coincidental `GC_FLAG_FORWARDED` byte makes the next
//! cycle's rewrite pass follow a garbage forwarding pointer — #8040, which cost
//! days to trace back from a `TypeError: value is not a function` in an
//! unrelated function. #8168 wired up the one table that had been missed, and
//! nothing was checking for the next one. [`DEAD_KEY_PRUNES`] below is the list
//! `fan_out` iterates, and `scripts/gc_rekeyed_key_tables.py` reads it: every
//! rekey site in the tree needs a written verdict in
//! `scripts/gc_rekeyed_key_tables.json`, a `dead_owner:` verdict must name a
//! prune that is actually in this array, and an exemption that matches nothing
//! fails too. Adding a rekeyed table without a prune is now a build failure.
//!
//! Note the ORDER on the copying minor: the rewrite pass runs BEFORE this
//! prune (`copying.rs` rewrites, then
//! `finalize_dead_copied_minor_from_space_side_allocations` prunes). A prune
//! therefore protects the NEXT cycle, which is sound only because the address
//! is not recycled until `copying_reset_from_spaces_and_flip`, strictly later
//! still.
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

/// Which of the pass's three deadness predicates a registered prune is handed.
///
/// Narrowing is not cosmetic: `Closure` and `Symbol` additionally require the
/// header's `obj_type` to match, which is what stops a prune from adjudicating
/// an address that has already been recycled as some other kind of object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeadKeyOwner {
    /// Any registered GC type.
    Any,
    /// `GC_TYPE_CLOSURE` only.
    Closure,
    /// `GC_TYPE_STRING` only — symbols are `gc_malloc`'d with that type.
    Symbol,
}

/// One address-keyed side table whose dead keys this pass drops.
pub(super) struct DeadKeyPrune {
    /// The declaration this prunes, as `scripts/gc_rekeyed_key_tables.py`
    /// names it. Several prunes cover more than one table; the label lists
    /// them so the registry reads as an inventory rather than a call list.
    /// Read by that script and by `gc::tests::dead_owner_side_tables`, neither
    /// of which is a compilation unit the dead-code pass can see.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) table: &'static str,
    pub(super) owner: DeadKeyOwner,
    pub(super) prune: fn(&dyn Fn(usize) -> bool),
}

/// THE REGISTRY (#8174).
///
/// `fan_out` iterates this instead of naming a dozen prunes inline, and
/// `scripts/gc_rekeyed_key_tables.py` reads it: a `dead_owner:<fn>` verdict in
/// `scripts/gc_rekeyed_key_tables.json` must name a prune that is in here, so
/// deleting an entry fails the gate rather than silently reopening #8040.
///
/// A table that is REKEYED (`RuntimeRootVisitor::visit_metadata_*` rewrites its
/// key without marking it) and is NOT in here has no death story, and a dead
/// key on such a table is not merely a leak — see `rewrite_raw_addr`'s #8174
/// note. The gate's job is to make that a build failure the day the table is
/// added, instead of a `TypeError: value is not a function` days later.
///
/// The converse is deliberately NOT required: three of these prune tables that
/// are re-keyed by a per-object move hook (`gc_type_after_payload_move`) rather
/// than by a metadata visitor, so they have no `visit_metadata_*` site at all.
pub(super) const DEAD_KEY_PRUNES: &[DeadKeyPrune] = &[
    DeadKeyPrune {
        table: "ARRAY_NAMED_PROPS",
        owner: DeadKeyOwner::Any,
        prune: crate::array::prune_dead_array_named_property_owners,
    },
    // Re-keyed by the per-object move hook, not by a metadata visitor.
    DeadKeyPrune {
        table: "ELEMENT_SHAPES",
        owner: DeadKeyOwner::Any,
        prune: crate::array::prune_dead_element_shape_owners,
    },
    DeadKeyPrune {
        table: "MAP_ITERATOR_ARRAYS",
        owner: DeadKeyOwner::Any,
        prune: crate::map::prune_dead_map_iterator_array_owners,
    },
    DeadKeyPrune {
        table: "SET_ITERATOR_ARRAYS",
        owner: DeadKeyOwner::Any,
        prune: crate::set::prune_dead_set_iterator_array_owners,
    },
    DeadKeyPrune {
        table: "state().descriptors.property_descriptors + .accessor_descriptors",
        owner: DeadKeyOwner::Any,
        prune: crate::object::prune_dead_descriptor_owner_entries,
    },
    DeadKeyPrune {
        table: "ARGUMENTS_OBJECTS",
        owner: DeadKeyOwner::Any,
        prune: crate::object::prune_dead_arguments_object_entries,
    },
    // Re-keyed by the per-object move hook, not by a metadata visitor.
    DeadKeyPrune {
        table: "OBJECT_PROTOTYPES",
        owner: DeadKeyOwner::Any,
        prune: crate::object::prototype_chain::prune_dead_object_prototype_owners,
    },
    // #6759 C1: shape records are keyed on keys_array addresses; drop the
    // ones whose keys_array died (memory only — per-hit validation covers
    // correctness for anything this misses).
    DeadKeyPrune {
        table: "state().shapes.inner (descriptors + indices)",
        owner: DeadKeyOwner::Any,
        prune: crate::object::shapes::prune_dead_shape_keys,
    },
    // Re-keyed by the per-object move hook, not by a metadata visitor.
    DeadKeyPrune {
        table: "state().exotic_expando.entries",
        owner: DeadKeyOwner::Any,
        prune: crate::object::exotic_expando::prune_dead_exotic_expando_owners,
    },
    DeadKeyPrune {
        table: "SYMBOL_PROPERTIES + SYMBOL_PROPERTY_ATTRS",
        owner: DeadKeyOwner::Any,
        prune: crate::symbol::prune_dead_symbol_property_owners,
    },
    DeadKeyPrune {
        table: "SYMBOL_POINTERS",
        owner: DeadKeyOwner::Symbol,
        prune: crate::symbol::prune_dead_symbol_pointers,
    },
    DeadKeyPrune {
        table: "CLOSURE_PROPS + CLOSURE_STATIC_PROTOTYPES + CLOSURE_DELETED_KEYS",
        owner: DeadKeyOwner::Closure,
        prune: crate::closure::prune_dead_closure_side_table_owners,
    },
    // #8040: `FUNCTION_CLASS_IDS` is keyed by a synthetic-class function
    // value's closure address, and is REKEYED (not re-derived) when that
    // closure moves — so a dead key does not merely leak, the rekey walk
    // follows whatever the recycled bytes at that address look like.
    DeadKeyPrune {
        table: "FUNCTION_CLASS_IDS",
        owner: DeadKeyOwner::Closure,
        prune: crate::object::prune_dead_function_class_id_keys,
    },
    DeadKeyPrune {
        table: "VM_CONTEXTS + VM_SCRIPTS + VM_FUNCTIONS",
        owner: DeadKeyOwner::Any,
        prune: crate::node_vm::prune_dead_vm_owner_entries,
    },
    DeadKeyPrune {
        table: "FILEHANDLE_OBJECT_FDS",
        owner: DeadKeyOwner::Any,
        prune: crate::fs::prune_dead_filehandle_fd_entries,
    },
];

fn fan_out(
    is_dead_owner: &dyn Fn(usize) -> bool,
    is_dead_closure: &dyn Fn(usize) -> bool,
    is_dead_symbol: &dyn Fn(usize) -> bool,
) {
    // Interned key pointers cached in the store-plan cache may die in this
    // collection — flush every cached verdict. Pointer identity only: the
    // entries pruned below all belong to objects that are DEAD, so no live
    // object's property lookup changes its answer. See
    // `prop_plan_gc_epoch_bump`.
    crate::object::prop_plan::prop_plan_gc_epoch_bump();
    for entry in DEAD_KEY_PRUNES {
        let is_dead: &dyn Fn(usize) -> bool = match entry.owner {
            DeadKeyOwner::Any => is_dead_owner,
            DeadKeyOwner::Closure => is_dead_closure,
            DeadKeyOwner::Symbol => is_dead_symbol,
        };
        (entry.prune)(is_dead);
    }
}
