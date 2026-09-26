//! Descriptor-owner lifecycle: what happens to a descriptor entry when its
//! OWNER ADDRESS dies, is cleared, or moves.
//!
//! Split out of `descriptor_state.rs` to keep that file under the 2000-line
//! size gate (`scripts/check_file_size.sh`). The five functions here are the
//! complete set that keys off an owner address changing or going away —
//! death pruning (full and minor-scoped), the shared entry removal they and
//! `clear_object_descriptors` share, the growth/evacuation re-key
//! (`transfer_descriptor_owner`), and the metadata-rewrite-phase address
//! translation `gc_scan.rs` applies to every owner it visits. The install /
//! lookup side of the same tables stays in the parent module.

use super::*;

/// Death pruning for the two descriptor side tables (2026-07-09 GC audit
/// wave 2). Entries are keyed by `(owner_addr, key)` and were never removed
/// when the owner died: `Object.freeze(perRequestObj)` leaked one entry per
/// key per request, accessor closures were immortalized by the root scanner
/// below, and a fresh object at a recycled address inherited the dead
/// owner's descriptors (stale "read only property" throws). `is_dead_owner`
/// is one of the GC's post-trace / copied-minor deadness predicates
/// (`gc::dead_owner`); each distinct owner is probed once.
pub(crate) fn prune_dead_descriptor_owner_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    let mut verdicts: HashMap<usize, bool> = HashMap::new();
    let mut is_dead = |owner: usize| -> bool {
        *verdicts
            .entry(owner)
            .or_insert_with(|| is_dead_owner(owner))
    };
    let st = state();
    {
        let mut m = st.descriptors.property_descriptors.borrow_mut();
        if !m.is_empty() {
            m.retain(|(owner, _), _| !is_dead(*owner));
        }
    }
    {
        let mut m = st.descriptors.accessor_descriptors.borrow_mut();
        if !m.is_empty() {
            m.retain(|(owner, _), _| !is_dead(*owner));
        }
    }
    // Keep the owner index in step: a dead owner left here would keep
    // reporting keys through `accessor_descriptor_keys_for_obj` after its
    // entries were reaped, and would be re-walked by every later GC scan.
    for index in [
        &st.descriptors.attr_keys_by_owner,
        &st.descriptors.accessor_keys_by_owner,
    ] {
        let mut idx = index.borrow_mut();
        if !idx.is_empty() {
            idx.retain(|owner, _| !is_dead(*owner));
        }
    }
}

/// [`prune_dead_descriptor_owner_entries`] for a MINOR (#9754): only a young
/// owner can be dead, and a young owner is always in the young log (noted at
/// insert, re-logged by every minor-scoped walk while it stays young), so the
/// log is the complete candidate set.
pub(crate) fn prune_dead_descriptor_owner_entries_young(is_dead_owner: &dyn Fn(usize) -> bool) {
    let st = state();
    let candidates = st.descriptors.young_owners.borrow_mut().take_sorted();
    let mut kept = Vec::with_capacity(candidates.len());
    for owner in candidates {
        if is_dead_owner(owner) {
            remove_descriptor_owner_entries(st, owner);
        } else {
            kept.push(owner);
        }
    }
    st.descriptors.young_owners.borrow_mut().extend(kept);
}

/// Drop every entry `owner` holds in both tables and both indexes, through
/// the owner index (O(owner's keys), not O(table)).
fn remove_descriptor_owner_entries(st: &crate::state::RuntimeState, owner: usize) {
    if let Some(keys) = st
        .descriptors
        .attr_keys_by_owner
        .borrow_mut()
        .remove(&owner)
    {
        let mut attrs = st.descriptors.property_descriptors.borrow_mut();
        for key in keys {
            attrs.remove(&(owner, key));
        }
    }
    if let Some(keys) = st
        .descriptors
        .accessor_keys_by_owner
        .borrow_mut()
        .remove(&owner)
    {
        let mut accessors = st.descriptors.accessor_descriptors.borrow_mut();
        for key in keys {
            accessors.remove(&(owner, key));
        }
    }
}

/// #6710: drop every property-attr + accessor descriptor owned by `obj`.
///
/// The generic descriptor tables are keyed by owner address; for a native
/// handle that address is its (recycled) handle id. `gc_sweep_dead_descriptors`
/// only reaps entries whose owner is a dead *heap* object, so a recycled handle
/// id's descriptors survive into the next owner. Called from
/// `handle_expando_clear` when perry-ffi hands a freed handle id back out.
///
/// RULE 1: this is a BULK descriptor removal — after it every key of the
/// receiver is an ordinary `{writable, enumerable, configurable}` data
/// property again — so a shaped ordinary receiver must transition, exactly as
/// the per-key [`clear_property_attrs`] / [`clear_accessor_descriptor`] do.
/// It used to make no shape call at all, so a cache primed while the object
/// was frozen kept serving the frozen answer. Today's only production caller
/// passes a handle-band id, which has no header and no shape (the transition
/// is then skipped by `object_is_shaped`), but nothing in the signature says
/// so and the function is reachable from anywhere in the crate.
///
/// The removal itself goes through the owner index rather than two O(table)
/// `retain` scans. That index is authoritative — every insert in this file
/// calls `owner_index_add`/`owner_index_push_proven_new` first, and
/// `gc_scan.rs` keeps it in step across evacuation — so it is both complete
/// and O(this owner's keys). It also removes the need for the
/// `HANDLE_HAS_DESCRIPTORS` latch that existed only to skip those scans, and
/// with it the silent no-op this function performed for a HEAP owner while no
/// handle had ever taken a descriptor.
pub(crate) fn clear_object_descriptors(obj: usize) {
    // Charter step 3: an ordinary object's attributes live with its keys; its
    // accessor closures still live in the tables and are dropped below.
    if unsafe { super::super::key_attrs::attrs_live_in_keys_for_install(obj) }
        && unsafe { super::super::key_attrs::object_summary(obj as *const ObjectHeader) } != 0
    {
        // The accessor pairs live in the keys' slots: clear those first, or
        // the slots would read as data once the entries say "default".
        for key in unsafe {
            super::super::key_attrs::object_accessor_key_names(obj as *const ObjectHeader)
        } {
            clear_accessor_descriptor(obj, &key);
        }
        super::super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
        note_descriptor_target_edits(obj, &[super::AttrsEdit::ClearAll]);
    }
    let st = state();
    let owned_any = st
        .descriptors
        .attr_keys_by_owner
        .borrow()
        .contains_key(&obj)
        || st
            .descriptors
            .accessor_keys_by_owner
            .borrow()
            .contains_key(&obj);
    if !owned_any {
        return;
    }
    remove_descriptor_owner_entries(st, obj);
    super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
    unsafe {
        let object = obj as *mut crate::object::ObjectHeader;
        if crate::object::object_is_shaped(object) {
            crate::object::shapes::transition_object_shape_semantics(object);
        }
    }
}

/// Move string-keyed descriptor ownership when `ArrayHeader` growth replaces
/// one live allocation with another. Array growth is not a GC collection, so
/// the metadata-rewrite scanner below does not run; without this explicit
/// transfer, descriptors installed before a later grow remain keyed to the
/// forwarding stub and disappear from reads through the canonical array head.
///
/// RULE 1: this makes NO shape call, and that is correct. It is not a
/// descriptor CHANGE — it is a RE-KEY of one logical object whose allocation
/// moved. The descriptor set, the attributes and the accessors are all
/// identical either side; only the address the tables index them under
/// changes. A shape transition here would be actively wrong: it would retire
/// every cache entry for a shape the object still has.
///
/// The contract that makes it correct is the CALLER's: the replacement cell
/// must already carry the original's identity — `js_array_grow` copies the
/// `GcHeader` (`_reserved`, hence `OBJ_FLAG_HAS_DESCRIPTORS`) verbatim before
/// calling here, and an array never carries the flag in the first place
/// (`note_descriptor_target_keyed` only sets it for `GC_TYPE_OBJECT`).
///
/// Where that contract is NOT met — any caller handing the entries to a cell
/// that is not already the same object — the moved descriptors would land on
/// a receiver whose header and shape both still say "no descriptors", which
/// is precisely what rule 1 forbids. Repair it rather than assert it: route
/// the new owner through the funnel, which sets the flag and transitions a
/// shaped ordinary receiver. Free on the array-growth path, where the
/// condition is false.
pub(crate) fn transfer_descriptor_owner(old_owner: usize, new_owner: usize) {
    if old_owner == new_owner {
        return;
    }
    let st = state();
    // The moved entries keep their accessor values, so the new owner is
    // logged unconditionally; the next minor-scoped walk drops it if nothing
    // in it is relevant any more.
    st.descriptors.young_owners.borrow_mut().note(new_owner);
    // The owner index names exactly this owner's keys, so neither table is
    // walked in full any more. Array growth calls this on every reallocation.
    {
        let moved = st
            .descriptors
            .attr_keys_by_owner
            .borrow()
            .get(&old_owner)
            .cloned()
            .unwrap_or_default();
        let mut attrs = st.descriptors.property_descriptors.borrow_mut();
        for key in moved {
            if let Some(value) = attrs.remove(&(old_owner, key.clone())) {
                attrs.insert((new_owner, key), value);
            }
        }
    }
    {
        let moved = st
            .descriptors
            .accessor_keys_by_owner
            .borrow()
            .get(&old_owner)
            .cloned()
            .unwrap_or_default();
        let mut accessors = st.descriptors.accessor_descriptors.borrow_mut();
        for key in moved {
            if let Some(value) = accessors.remove(&(old_owner, key.clone())) {
                accessors.insert((new_owner, key), value);
            }
        }
    }
    owner_index_transfer(&st.descriptors.attr_keys_by_owner, old_owner, new_owner);
    owner_index_transfer(&st.descriptors.accessor_keys_by_owner, old_owner, new_owner);

    // Carry the per-object Bloom summary across too. Every descriptor read is
    // gated on the owner's `attr_key_bits` / `accessor_key_bits`
    // (`owner_may_have_descriptor_entries`), and a freshly grown array has a
    // null `meta` — for which that gate answers **false**, authoritatively.
    // Without this the entries move correctly and then read back as absent:
    // `Object.keys` / `getOwnPropertyDescriptor` silently lose every accessor
    // an array had before it grew. (Pre-existing: the gate sat in front of the
    // old full-table scan as well, so the scan never ran for the new owner.)
    //
    // Done after the borrows above are released — `note_meta_descriptor_key`
    // allocates via `object_meta_ensure`.
    let moved_attr = st
        .descriptors
        .attr_keys_by_owner
        .borrow()
        .get(&new_owner)
        .cloned()
        .unwrap_or_default();
    let moved_acc = st
        .descriptors
        .accessor_keys_by_owner
        .borrow()
        .get(&new_owner)
        .cloned()
        .unwrap_or_default();
    for key in &moved_attr {
        note_meta_descriptor_key(new_owner, key, false);
    }
    for key in &moved_acc {
        note_meta_descriptor_key(new_owner, key, true);
    }

    // Rule 1 repair (see the doc comment): a new owner that did not already
    // carry the source's descriptor state must not silently acquire
    // descriptors behind an unchanged header and shape.
    if object_has_descriptors(old_owner) && !object_has_descriptors(new_owner) {
        note_descriptor_target_edits(new_owner, &[]);
    }
}

/// Rewrite a descriptor table's owner ADDRESS during the GC metadata-rewrite
/// phase (evacuation moved the owning object), mirroring the symbol-keyed
/// twin tables' owner rekey (`symbol/gc_roots.rs`). Outside that phase the
/// owner is returned unchanged.
pub(super) fn rewrite_descriptor_owner(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
) -> usize {
    if !visitor.is_metadata_rewrite_phase() {
        return owner;
    }
    let mut addr = owner;
    visitor.visit_metadata_usize_slot(&mut addr);
    addr
}
