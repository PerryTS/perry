//! Remembered-set scanning and the pinned-young preflight for the copying
//! minor, split out of `copying.rs` for the 2000-line file cap.
//!
//! A child module, so `use super::*` reaches the parent's private items.

use super::*;

pub(in crate::gc) fn scan_remembered_dirty_slots_copying(
    snapshot: &RememberedDirtySnapshot,
    mut covered: Option<&mut crate::fast_hash::PtrHashSet<usize>>,
    mut visit: impl FnMut(*mut u64, *mut GcHeader, bool, &mut RememberedSetTraceStats),
) -> RememberedSetTraceStats {
    let mut stats = RememberedSetTraceStats {
        entries_scanned: snapshot.dirty_old_pages.len()
            + snapshot.external_dirty_entries.len()
            + snapshot.fallback_headers.len(),
        dirty_pages_before: snapshot.dirty_pages.len(),
        dirty_pages_scanned: snapshot.dirty_pages.len(),
        ..RememberedSetTraceStats::default()
    };
    let mut seen_headers = crate::fast_hash::new_ptr_hash_set();

    let mut scan_header = |header: *mut GcHeader, stats: &mut RememberedSetTraceStats| unsafe {
        if header.is_null() || !seen_headers.insert(header as usize) {
            return;
        }
        let arena_parent = plausible_gc_header(header, true);
        let malloc_parent = !arena_parent && plausible_gc_header(header, false);
        if !arena_parent && !malloc_parent {
            return;
        }
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        if arena_parent
            && !matches!(
                crate::arena::classify_heap_generation(user),
                crate::arena::HeapGeneration::Old
            )
        {
            return;
        }
        stats.old_objects_considered += 1;
        stats.valid_roots += 1;
        stats.dirty_objects_scanned += 1;
        let mut changed = false;
        let mut visit_slot = |slot: *mut u64, stats: &mut RememberedSetTraceStats| {
            let external = !matches!(
                crate::arena::classify_heap_generation(slot as usize),
                crate::arena::HeapGeneration::Old
            );
            let before = *slot;
            visit(slot, header, external, stats);
            changed |= *slot != before;
        };
        let complete =
            scan_dirty_object_slots(header, &snapshot.dirty_pages, stats, &mut visit_slot);
        if complete {
            if let Some(covered) = covered.as_deref_mut() {
                covered.insert(header as usize);
            }
        }
        if changed {
            run_gc_rewrite_hook((*header).obj_type, user);
        }
    };

    if !snapshot.dirty_old_pages.is_empty() {
        crate::arena::old_arena_walk_objects_on_pages(&snapshot.dirty_old_pages, |header| {
            scan_header(header as *mut GcHeader, &mut stats);
        });
    }
    for &(_, header_addr) in &snapshot.external_dirty_entries {
        scan_header(header_addr as *mut GcHeader, &mut stats);
    }
    for header_addr in snapshot.fallback_headers.iter().copied() {
        scan_header(header_addr as *mut GcHeader, &mut stats);
    }

    stats.dirty_pages_after = remembered_dirty_page_count();
    stats
}

/// The young-pin latch was clear, the preflight was skipped on that proof, and
/// the copier then met bytes that describe a pinned young object anyway.
/// This is the instant relocation would become unsafe, but it does not by
/// itself identify the violated invariant. In #7990 the header was internally
/// impossible (`GC_TYPE_MAP | GC_FLAG_INTERNED`), and the fault disappeared
/// when comparison operands were rooted; the pin latch itself was complete.
///
/// There is no recovery: leaving the object in from-space strands the
/// referring slot on memory `copying_reset_from_spaces_and_flip` is about to
/// retire, and moving it invalidates a raw address nothing will rewrite. Abort
/// loudly at the faulting site instead of corrupting the heap silently.
#[cold]
#[inline(never)]
pub(in crate::gc) unsafe fn pinned_young_move_under_skipped_preflight(header: *mut GcHeader) -> ! {
    // #7990: the report is built in `gc/pin.rs` from the header's own flags,
    // because those flags are the only evidence that distinguishes an
    // incomplete pin latch from a dangling pointer into recycled memory — and
    // this message used to assert the former as fact while `gc_pin_sites.py`,
    // the tool it told the reader to run, answered OK.
    eprintln!(
        "{}",
        super::pin::pinned_young_move_report(
            header as usize,
            (*header).obj_type,
            (*header).size,
            (*header).gc_flags,
        )
    );
    std::process::abort()
}
