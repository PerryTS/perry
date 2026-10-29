use super::*;

/// Follow forwarding pointers for a word that may hold a heap reference,
/// NaN-boxed or bare, preserving the form it was stored in.
///
/// Decoding goes through [`decode_root_word`] so this path and the *mark*
/// path (`mark_mutable_root_bits`) accept exactly the same bit patterns —
/// see `gc::root_words` for why relocating a word the mark path would have
/// skipped is a use-after-free (#6910).
pub(super) fn try_rewrite_value(bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let word = decode_root_word(bits)?;
    let new_user = try_rewrite_raw_addr(word.addr(), valid_ptrs)?;
    Some(word.encode(new_user))
}

pub(super) fn try_rewrite_nanboxed_value(bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let word = decode_nanboxed_root_word(bits)?;
    Some(word.encode(try_rewrite_raw_addr(word.addr(), valid_ptrs)?))
}

/// #8174: refuses a forwarding target that is not a heap object start, in
/// lockstep with [`CopyingNurseryCollector::rewrite_raw_addr`](super::copying).
///
/// The lockstep is the point. The VERIFY pass shares this forwarding walker
/// and panics when it finds a stale alias the rewrite pass left alone.
/// Copying verification permits retained array-growth aliases through
/// [`EvacuationVerifier`], without changing source/target validation.
/// Tightening only the rewrite pass would
/// therefore have turned a silently-corrupt rewrite into a `PERRY_GC_VERIFY_
/// EVACUATION` abort blaming an innocent scanner — the two walkers must reach
/// the same verdict or the verifier is measuring the difference between them
/// instead of the heap. Its own gate (`valid_ptrs`, a live census) is strictly
/// stronger than the copier's heap-region test, so this only changes the case
/// where a genuinely LIVE forwarded object's target word is corrupt.
pub(super) fn try_rewrite_raw_addr(ptr_addr: usize, valid_ptrs: &ValidPointerSet) -> Option<usize> {
    follow_forwarding_raw_addr(ptr_addr, valid_ptrs, |_, _| true)
}

pub(super) fn follow_forwarding_raw_addr(
    ptr_addr: usize,
    valid_ptrs: &ValidPointerSet,
    must_rewrite: impl Fn(usize, usize) -> bool,
) -> Option<usize> {
    if ptr_addr == 0 {
        return None;
    }
    let mut current = ptr_addr;
    let mut rewrote = false;
    for _ in 0..64 {
        if !valid_ptrs.contains(&current) {
            return rewrote.then_some(current);
        }
        unsafe {
            let header = (current as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            if (*header).gc_flags & GC_FLAG_FORWARDED == 0 {
                return rewrote.then_some(current);
            }
            let next = forwarding_address(header) as usize;
            if next == 0 || next == current {
                return rewrote.then_some(current);
            }
            if !accept_forwarding_target(next) {
                return None;
            }
            rewrote |= must_rewrite(current, next);
            current = next;
        }
    }
    rewrote.then_some(current)
}

#[inline]
pub(super) unsafe fn rewrite_mutable_slot(slot: GcMutableSlot, valid_ptrs: &ValidPointerSet) {
    if let Some(bits) = try_rewrite_value(slot.read(), valid_ptrs) {
        slot.write(bits);
    }
}

pub(super) unsafe fn rewrite_heap_object_fields<const DIAGNOSTICS: bool>(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
) {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let mut changed = false;
    visit_gc_rewrite_slots::<DIAGNOSTICS>(header, |slot| unsafe {
        if DIAGNOSTICS {
            slot.record_layout_read()
        };
        let before = slot.read();
        rewrite_mutable_slot(slot, valid_ptrs);
        changed |= slot.read() != before;
    });
    if changed {
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        run_gc_rewrite_hook((*header).obj_type, user_ptr as usize);
    }
}

/// Walk every live (MARKED, non-FORWARDED) object on the heap and
/// rewrite any forwarded references in its fields. Includes new
/// evac copies (marked at evac time) and surviving non-evacuated
/// objects.
pub(super) fn rewrite_heap_objects<const DIAGNOSTICS: bool>(valid_ptrs: &ValidPointerSet) {
    let rewrite_one = |header: *mut GcHeader| {
        unsafe {
            let flags = (*header).gc_flags;
            // FORWARDED originals are stale — first 8 bytes of
            // payload now holds the forwarding address, not real
            // field data. Skip them entirely.
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            // Skip dead NURSERY objects — this cycle's sweep frees them.
            // An UNMARKED object outside the nursery is NOT dead: a minor
            // cycle neither traces nor sweeps the old generation, so being
            // unmarked is the normal state of a live old object whose pages
            // are clean. Old→old references have no dirty-page coverage
            // (barriers only track old→young), which makes this walk the
            // ONLY pass that re-points an old referrer at an old-page
            // evacuation target. Skipping unmarked old objects left their
            // slots aimed at forwarding stubs that
            // `release_evacuated_original_forwarding_stubs` then released —
            // dangling pointers into reused memory (#5029).
            if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
                if crate::arena::pointer_in_nursery(user) {
                    return;
                }
            }
            rewrite_heap_object_fields::<DIAGNOSTICS>(header, valid_ptrs);
        }
    };
    crate::arena::arena_walk_objects(|hp| rewrite_one(hp as *mut GcHeader));
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &h in s.objects.iter() {
            rewrite_one(h);
        }
    });
}

pub(super) fn rewrite_remembered_dirty_ranges<const DIAGNOSTICS: bool>(
    valid_ptrs: &ValidPointerSet,
) {
    let snapshot = remembered_dirty_snapshot::<DIAGNOSTICS>();
    let mut stats = RememberedSetTraceStats::default();
    let mut rewrite_dirty_slot = |slot: GcMutableSlot, _stats: &mut RememberedSetTraceStats| unsafe {
        rewrite_mutable_slot(slot, valid_ptrs);
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut rewrite_dirty_slot);

    for header_addr in snapshot.fallback_headers {
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        unsafe {
            rewrite_heap_object_fields::<DIAGNOSTICS>(header_addr as *mut GcHeader, valid_ptrs);
        }
    }
}

/// Walk every mutable root slot and rewrite forwarded pointers.
///
/// The source owner selects the same decoder for marking and rewriting:
/// global/native homes contain JSValues; untyped shadow homes may also contain
/// bare pointers. Never relocate a numeric precise value as a pointer (#6910).
pub(super) fn rewrite_mutable_root_slots(
    valid_ptrs: &ValidPointerSet,
    shadow_stats: Option<&mut ShadowRootTraceStats>,
) {
    rewrite_mutable_root_slots_with_sources(valid_ptrs, shadow_stats, None);
}

pub(super) fn rewrite_mutable_root_slots_with_sources(
    valid_ptrs: &ValidPointerSet,
    mut shadow_stats: Option<&mut ShadowRootTraceStats>,
    mut root_sources: Option<&mut RootSourcesTraceStats>,
) {
    let native_stack_walk = visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        record_mutable_slot_scan_source(slot, bits, valid_ptrs, &mut root_sources);
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = slot
            .pointer_word(bits)
            .and_then(|_| try_rewrite_value(bits, valid_ptrs))
        {
            slot.write(new_bits);
            record_mutable_slot_rewrite_source(slot, &mut root_sources);
            if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                if let Some(stats) = shadow_stats.as_mut() {
                    stats.record_rewrite();
                }
            }
        }
    });
    record_native_stack_walk_source(native_stack_walk, &mut root_sources);
}

pub(super) fn rewrite_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    rewrite_mutable_registered_roots_with_sources(valid_ptrs, None);
}

pub(super) fn rewrite_mutable_registered_roots_with_sources(
    valid_ptrs: &ValidPointerSet,
    mut root_sources: Option<&mut RootSourcesTraceStats>,
) {
    let scanners: Vec<MutableRootScannerEntry> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut visitor = RuntimeRootVisitor::for_rewrite(valid_ptrs);
    for entry in scanners {
        let stats = match &mut root_sources {
            Some(sources) => match entry.source {
                MutableRootScannerSource::RuntimeHandles => {
                    Some(&mut sources.runtime_handles as *mut RootSourceSlotTraceStats)
                }
                MutableRootScannerSource::RuntimeMutableScanner => {
                    Some(&mut sources.runtime_mutable_scanners as *mut RootSourceSlotTraceStats)
                }
            },
            None => None,
        };
        let previous = visitor.set_root_source_stats(stats);
        (entry.scanner)(&mut visitor);
        visitor.set_root_source_stats(previous);
    }
    visit_ffi_mutable_registered_roots_with_sources(&mut visitor, root_sources);
}

/// Top-level Phase C4b-γ-2 entry: rewrite every reference site we
/// own. Skipped: conservatively-discovered C-stack words (we can't
/// safely overwrite arbitrary stack memory; pinning of conservative-
/// root targets in `gc_collect_minor` keeps those references valid
/// without rewriting). Legacy copy-only scanners still pin their own
/// discoveries directly during root marking.
pub(super) fn rewrite_forwarded_references<const DIAGNOSTICS: bool>(
    valid_ptrs: &ValidPointerSet,
    shadow_stats: Option<&mut ShadowRootTraceStats>,
    root_sources: Option<&mut RootSourcesTraceStats>,
) {
    match root_sources {
        Some(sources) => {
            rewrite_mutable_root_slots_with_sources(valid_ptrs, shadow_stats, Some(&mut *sources));
            rewrite_mutable_registered_roots_with_sources(valid_ptrs, Some(&mut *sources));
        }
        None => {
            rewrite_mutable_root_slots(valid_ptrs, shadow_stats);
            rewrite_mutable_registered_roots(valid_ptrs);
        }
    }
    rewrite_remembered_dirty_ranges::<DIAGNOSTICS>(valid_ptrs);
    super::image_cells::rewrite_image_roots(valid_ptrs);
    rewrite_heap_objects::<DIAGNOSTICS>(valid_ptrs);
}

/// Gen-GC Phase C4b: is `header` pinned this cycle (cannot be
/// evacuated)? Tested by the evacuation candidate filter in
/// `gc_collect_minor` after the age-bump pass.
#[inline]
pub fn is_conservatively_pinned(header: *const GcHeader) -> bool {
    CONS_PINNED.with(|s| s.borrow().contains(&(header as usize)))
}

/// Test-only diagnostic: number of objects pinned this cycle.
pub fn cons_pinned_count() -> usize {
    CONS_PINNED.with(|s| s.borrow().len())
}
