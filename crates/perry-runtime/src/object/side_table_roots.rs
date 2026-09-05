//! GC root scanning and dead-owner pruning for the two hot shape caches.
//!
//! The transition cache (`(predecessor ShapeId, key) -> successor`) and the
//! shape cache (`static shape id -> canonical keys array`) both hold raw
//! `*mut ArrayHeader` pointers that no live object need hold directly, so the
//! collector has to visit them or the next cache hit dereferences freed
//! memory. Both carry a young-entry log (`gc/young_log.rs`), so each scanner
//! and each prune exists in a full-walk and a minor-scoped form; keeping the
//! four pairs together — and out of `object/mod.rs`, which is at the
//! file-size gate — is what this module is for.
//!
//! Everything here reaches its tables through `super`: the caches, their
//! logs and their per-entry helpers stay private to `object`.

use super::*;

/// GC root scanner for the transition cache. Same contract as
/// `scan_shape_cache_roots` — without this the mark phase would free
/// cached target arrays that no live object currently holds directly,
/// and the next cache-hit store would dereference freed memory.
///
/// #855: walk the static via `&raw const` + raw pointer indexing to
/// avoid the `static_mut_refs` lint (hard error in Rust 2024). The
/// cache is thread-local-by-discipline (perry user code is single-
/// threaded), so the unsafe deref is sound.
pub fn scan_transition_cache_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_transition_cache_roots_mut(&mut visitor);
}

pub fn scan_transition_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // #9754: a minor-scoped pass visits only the young-logged slots; a full
    // pass walks the table and rebuilds the log. Both share
    // `scan_transition_cache_slot`.
    if visitor.young_scope() {
        let mut logged = 0u64;
        let mut visited = 0u64;
        let mut kept = Vec::new();
        #[cfg(debug_assertions)]
        with_transition_cache(|table| unsafe {
            let relevant: Vec<u32> = (0..TRANSITION_CACHE_SIZE)
                .filter(|&i| transition_entry_is_minor_relevant(&(*table)[i]))
                .map(|i| i as u32)
                .collect();
            TRANSITION_CACHE_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(TRANSITION_CACHE_YOUNG_LOG_NAME, &relevant)
            });
        });
        let batch = TRANSITION_CACHE_YOUNG.with(|log| log.borrow_mut().take_sorted());
        logged += batch.len() as u64;
        with_transition_cache(|table| unsafe {
            for slot in batch {
                visited += 1;
                if scan_transition_cache_slot(visitor, table, slot as usize) {
                    kept.push(slot);
                }
            }
        });
        let kept_len = kept.len() as u64;
        TRANSITION_CACHE_YOUNG.with(|log| log.borrow_mut().extend(kept));
        crate::gc::young_log::note_walk(
            TRANSITION_CACHE_YOUNG_LOG_NAME,
            crate::gc::young_log::YoungLogWalk {
                partial: true,
                logged,
                visited,
                kept: kept_len,
                table_len: TRANSITION_CACHE_SIZE as u64,
            },
        );
        array_tail_transition::scan_roots_mut(visitor);
        return;
    }
    let _ = TRANSITION_CACHE_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let mut kept = Vec::new();
    with_transition_cache(|table| unsafe {
        for i in 0..TRANSITION_CACHE_SIZE {
            if scan_transition_cache_slot(visitor, table, i) {
                kept.push(i as u32);
            }
        }
    });
    let kept_len = kept.len() as u64;
    TRANSITION_CACHE_YOUNG.with(|log| log.borrow_mut().extend(kept));
    crate::gc::young_log::note_walk(
        TRANSITION_CACHE_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: false,
            logged: TRANSITION_CACHE_SIZE as u64,
            visited: TRANSITION_CACHE_SIZE as u64,
            kept: kept_len,
            table_len: TRANSITION_CACHE_SIZE as u64,
        },
    );
    array_tail_transition::scan_roots_mut(visitor);
}

/// Visit one transition-cache slot. Returns whether the entry can still
/// matter to a minor afterwards.
///
/// # Safety
/// `table` must be this thread's transition cache.
unsafe fn scan_transition_cache_slot(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    table: *mut [TransitionEntry; TRANSITION_CACHE_SIZE],
    i: usize,
) -> bool {
    let entry = &mut (*table)[i];
    if entry.next_keys == 0 {
        return false;
    }
    let mut invalidate = false;
    // Content-namespace ids (len marker != 0) are string BYTES,
    // not addresses — the visitor must not rewrite them.
    if (entry.slot_idx >> 24) == 0 {
        invalidate |= visitor.visit_metadata_usize_slot(&mut entry.key_ptr);
    }
    // #6759 phase 3: `next_keys` is WEAK, not a strong root.
    //
    // `visit_usize_slot` MARKS. With 16384 slots this cache was
    // therefore keeping up to 16384 keys arrays — and, through
    // them, their shape descriptors — alive whether or not any live
    // object still had that shape. That is a direct contributor to
    // the shape table growing without bound between full
    // collections (measured: 786k descriptors on a workload holding
    // under 400 live objects).
    //
    // A transition entry is a pure cache: it answers "adding key k
    // to shape S yields shape T". If nothing has shape T any more,
    // the answer is worthless, so pinning T's keys array to keep it
    // answerable is backwards. `key_ptr` was already weak for the
    // same reason; this makes the pair consistent.
    //
    // Rewrite-only keeps a surviving target's address correct;
    // `prune_dead_transition_cache_entries` drops the entry when the
    // target did not survive.
    visitor.visit_metadata_usize_slot(&mut entry.next_keys);
    if invalidate {
        *entry = TransitionEntry {
            key_ptr: 0,
            next_keys: 0,
            prev_shape_id: 0,
            target_shape_id: 0,
            slot_idx: 0,
            target_len: 0,
        };
        return false;
    }
    transition_entry_is_minor_relevant(entry)
}

/// [`prune_dead_transition_cache_entries`] for a MINOR (#9754): only a slot
/// in the young log can name a young — hence possibly dead — address.
#[cold]
pub(crate) fn prune_dead_transition_cache_entries_young(is_dead_owner: &dyn Fn(usize) -> bool) {
    let candidates = TRANSITION_CACHE_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let mut kept = Vec::with_capacity(candidates.len());
    with_transition_cache(|table| unsafe {
        for slot in candidates {
            let entry = &mut (*table)[slot as usize];
            if entry.next_keys == 0 {
                continue;
            }
            if transition_entry_is_dead(entry, is_dead_owner) {
                *entry = TransitionEntry {
                    key_ptr: 0,
                    next_keys: 0,
                    prev_shape_id: 0,
                    target_shape_id: 0,
                    slot_idx: 0,
                    target_len: 0,
                };
            } else {
                kept.push(slot);
            }
        }
    });
    TRANSITION_CACHE_YOUNG.with(|log| log.borrow_mut().extend(kept));
    array_tail_transition::prune_invalid_entries();
}

/// The death test of `prune_dead_transition_cache_entries`, shared with the
/// young-only variant.
fn transition_entry_is_dead(
    entry: &TransitionEntry,
    is_dead_owner: &dyn Fn(usize) -> bool,
) -> bool {
    ((entry.slot_idx >> 24) == 0 && entry.key_ptr != 0 && is_dead_owner(entry.key_ptr))
        // #6759 phase 3: `next_keys` stopped being a strong root, so a
        // dead target is now possible and must be reaped here — this is
        // the half that makes weakening it safe.
        || is_dead_owner(entry.next_keys)
        || shapes::shape_descriptor_by_id(entry.prev_shape_id).is_none()
        || (entry.target_shape_id != 0
            && shapes::shape_descriptor_by_id(entry.target_shape_id).is_none())
}

/// #8192: death pruning for the transition cache.
///
/// The interned `key_ptr` is metadata-only and therefore weak; `next_keys` is
/// a strong root. The predecessor and target ShapeIds are stable non-pointer
/// metadata, so moving collection neither rewrites nor invalidates them.
///
/// The entry is a pure cache, so the repair is to drop it. `next_keys == 0` is
/// the empty-slot sentinel.
///
/// `gc::dead_owner::DEAD_KEY_PRUNES` runs `prune_dead_shape_keys` before this
/// function. A predecessor whose keys edge died therefore has no descriptor
/// by the time we visit the cache. Both ShapeIds must still resolve: the
/// predecessor is weak, while the strongly rooted target keys normally keep
/// their descriptor live. Checking both here makes that target invariant a
/// release-mode post-GC proof without adding a hash-table lookup to every hot
/// transition stamp.
#[cold]
pub(crate) fn prune_dead_transition_cache_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    with_transition_cache(|table| unsafe {
        for i in 0..TRANSITION_CACHE_SIZE {
            let entry = &mut (*table)[i];
            if entry.next_keys == 0 {
                continue;
            }
            if transition_entry_is_dead(entry, is_dead_owner) {
                *entry = TransitionEntry {
                    key_ptr: 0,
                    next_keys: 0,
                    prev_shape_id: 0,
                    target_shape_id: 0,
                    slot_idx: 0,
                    target_len: 0,
                };
            }
        }
    });
    array_tail_transition::prune_invalid_entries();
}

#[cfg(test)]
pub(crate) fn test_transition_cache_occupancy() -> usize {
    with_transition_cache(|table| unsafe {
        (0..TRANSITION_CACHE_SIZE)
            .filter(|&i| (*table)[i].next_keys != 0)
            .count()
    })
}

/// Drive the PRODUCTION transition-cache writer from a test. Nothing but a
/// call, so it cannot drift from the writer it stands in for.
#[cfg(test)]
pub(crate) fn test_transition_cache_insert(
    prev_shape_id: u32,
    interned_key: *const crate::StringHeader,
    next_keys: usize,
    slot_idx: u32,
    target_shape_id: u32,
) {
    super::transition_cache_insert(
        std::ptr::null(),
        prev_shape_id,
        interned_key,
        next_keys,
        slot_idx,
        target_shape_id,
    );
}

#[cfg(test)]
pub(crate) fn test_seed_transition_cache_entry(
    prev_shape_id: u32,
    key_ptr: usize,
    next_keys: usize,
) {
    let slot = transition_cache_slot(prev_shape_id, key_ptr);
    // The seed writes `slot_idx: 0`, i.e. no length marker, so `key_ptr` is an
    // address here — which is the `len_marker == 0` arm of the shared predicate.
    arm_transition_cache_young(slot, next_keys, key_ptr, 0);
    with_transition_cache(|table| unsafe {
        (*table)[slot] = TransitionEntry {
            key_ptr,
            next_keys,
            prev_shape_id,
            target_shape_id: 0,
            slot_idx: 0,
            target_len: 1,
        };
    });
}

/// GC root scanner: mark all cached shape keys arrays so they're not freed.
/// The inline cache + overflow map both hold the raw `*mut ArrayHeader`
/// pointers; without this scanner, GC would free those arrays, leaving
/// every object with that shape holding a dangling `keys_array` pointer.
pub fn scan_shape_cache_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_shape_cache_roots_mut(&mut visitor);
}

pub fn scan_shape_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    use crate::gc::young_log::addr_is_minor_relevant;
    let st = crate::state::state();
    // The inline array is 256 fixed slots: always walked. The overflow map
    // holds every shape id ever cached; #9754: a minor-scoped pass visits
    // only the young-logged ids there, a full pass rebuilds the log.
    let entries = unsafe { &mut *st.object_hot.shape_inline_cache.get() };
    for entry in entries.iter_mut() {
        visitor.visit_raw_mut_ptr_slot(&mut entry.keys_array);
    }
    let mut cache = st.object_hot.shape_cache_overflow.borrow_mut();
    let table_len = cache.len() as u64;
    if visitor.young_scope() {
        #[cfg(debug_assertions)]
        {
            let relevant: Vec<u32> = cache
                .iter()
                .filter(|(_, (arr_ptr, _))| addr_is_minor_relevant(*arr_ptr as usize))
                .map(|(&id, _)| id)
                .collect();
            SHAPE_CACHE_YOUNG.with(|log| {
                log.borrow()
                    .debug_assert_logged(SHAPE_CACHE_YOUNG_LOG_NAME, &relevant)
            });
        }
        let batch = SHAPE_CACHE_YOUNG.with(|log| log.borrow_mut().take_sorted());
        let logged = batch.len() as u64;
        let mut kept = Vec::new();
        for id in batch {
            if let Some((arr_ptr, _)) = cache.get_mut(&id) {
                visitor.visit_raw_mut_ptr_slot(arr_ptr);
                if addr_is_minor_relevant(*arr_ptr as usize) {
                    kept.push(id);
                }
            }
        }
        let kept_len = kept.len() as u64;
        SHAPE_CACHE_YOUNG.with(|log| log.borrow_mut().extend(kept));
        crate::gc::young_log::note_walk(
            SHAPE_CACHE_YOUNG_LOG_NAME,
            crate::gc::young_log::YoungLogWalk {
                partial: true,
                logged,
                visited: logged,
                kept: kept_len,
                table_len,
            },
        );
        return;
    }
    let _ = SHAPE_CACHE_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let mut kept = Vec::new();
    for (&id, (arr_ptr, _runtime_shape_id)) in cache.iter_mut() {
        visitor.visit_raw_mut_ptr_slot(arr_ptr);
        if addr_is_minor_relevant(*arr_ptr as usize) {
            kept.push(id);
        }
    }
    let kept_len = kept.len() as u64;
    SHAPE_CACHE_YOUNG.with(|log| log.borrow_mut().extend(kept));
    crate::gc::young_log::note_walk(
        SHAPE_CACHE_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: false,
            logged: table_len,
            visited: table_len,
            kept: kept_len,
            table_len,
        },
    );
}
