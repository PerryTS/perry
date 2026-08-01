//! Whole-heap from-space scan (#7035).
//!
//! # Why this exists
//!
//! `PERRY_GC_VERIFY_EVACUATION` walks the same surfaces the *rewrite pass*
//! walks — `visit_shadow_stack_root_slots`, `visit_global_root_slots`, and the
//! registered side-table scanners. Both derive from one enumeration, so the
//! verifier can only check that the rewrite pass agreed with itself: a holder
//! the rewrite pass never knew about is invisible to the verifier too. On the
//! #7022 reproducer the verifier reports clean while the program dies of malloc
//! heap corruption, which is exactly that blind spot.
//!
//! This pass takes the opposite approach and asks a question that does not
//! depend on any root enumeration:
//!
//! > After the rewrite pass, does any live object OUTSIDE from-space still
//! > contain a word that decodes to a from-space address?
//!
//! It answers it by walking the whole heap — the arena census (`Address`-order
//! `ArenaObjectCursor`, covering Eden / both survivor semispaces / old / long-
//! lived) plus the malloc-tracked registry — and decoding every payload word
//! through `decode_root_word`, the same decoder the mark and rewrite paths
//! share (#6910). Objects that are themselves in from-space are skipped: they
//! are about to be reclaimed, and a dead object legitimately still points at
//! its dead peers. So are owners carrying `GC_FLAG_FORWARDED` — a relocation
//! stub is dead for the same reason, one step further on, and unlike a
//! from-space object it can sit in old-gen where the space check does not reach
//! it. Both skips are counted (`fwd_owners_skipped=`) so a suppressed
//! population can never read as a fixed one.
//!
//! A word whose target carries `GC_FLAG_FORWARDED` is an unambiguous **missing
//! rewrite**: the object moved this cycle and this reference was not updated.
//! A word pointing at a non-forwarded from-space object is a **dangling**
//! reference — the target was not evacuated at all, so it is about to be
//! recycled underneath this holder.
//!
//! # Cost and placement
//!
//! O(live heap) per collection, so it is strictly a debug instrument behind
//! `PERRY_GC_FROMSPACE_SCAN=1`. It must run after the rewrite pass and
//! **before** `copying_reset_from_spaces_and_flip`, while from-space is still
//! intact and page-registered — the same window the evacuation verifier uses.
//!
//! `PERRY_GC_FROMSPACE_SCAN_ABORT=1` additionally aborts on the first offender
//! so a debugger/crash report captures the offending cycle rather than the
//! downstream corruption.

use super::*;

/// One offending word found by the scan.
#[derive(Clone, Copy)]
pub(crate) struct FromSpaceRef {
    /// Header of the object that CONTAINS the stale word.
    pub(super) owner_header: usize,
    pub(super) owner_obj_type: u8,
    pub(super) owner_space: crate::arena::HeapSpace,
    /// Byte offset of the stale word within the owner's payload.
    pub(super) slot_offset: usize,
    /// The from-space address the word decodes to.
    pub(super) target: usize,
    pub(super) target_space: crate::arena::HeapSpace,
    /// True when the target carries `GC_FLAG_FORWARDED` — i.e. it MOVED and
    /// this reference was simply not rewritten.
    pub(super) target_forwarded: bool,
    /// True when the word was NaN-boxed, false when it was a bare address.
    pub(super) nanboxed: bool,
    /// Remembered-set coverage of THIS SLOT's page, the decisive split:
    /// `ever_dirty == false` -> a store path skipped the write barrier;
    /// `ever_dirty && !dirty_now` -> the edge was recorded and then LOST.
    pub(super) slot_dirty_now: bool,
    pub(super) slot_ever_dirty: bool,
    /// Owner's raw `gc_flags`, reported uninterpreted. NOTE: during a minor,
    /// old-gen parents are NOT marked even when their slots are scanned through
    /// the remembered set, so `GC_FLAG_MARKED` here does not mean "was scanned".
    pub(super) owner_flags: u8,
}

#[derive(Default)]
pub(crate) struct FromSpaceScanReport {
    pub(crate) objects_scanned: usize,
    pub(crate) words_scanned: usize,
    /// Owners skipped because they are themselves FORWARDED (dead relocation
    /// stubs). Reported so the filter can never be mistaken for a fix.
    pub(crate) forwarded_owners_skipped: usize,
    pub(crate) missing_rewrites: usize,
    pub(crate) dangling: usize,
    /// Offending slots whose page was NEVER dirtied -> a store path skipped the
    /// write barrier entirely.
    pub(crate) never_dirty: usize,
    /// Offending slots whose page was dirtied at some point but is not dirty
    /// now -> the edge was recorded and then lost by a clear/restore gap.
    pub(crate) lost_dirty: usize,
    /// Offending slots whose page IS currently dirty -> the remembered set had
    /// it and the scan still missed the slot.
    pub(crate) dirty_but_missed: usize,
    /// Offending slots whose page was NOT in this cycle's dirty snapshot, so
    /// the in-cycle remembered-set scan never looked at them.
    pub(crate) not_in_snapshot: usize,
    /// Offending slots whose page WAS in the snapshot -- the scan looked at the
    /// page and still did not rewrite the slot.
    pub(crate) in_snapshot: usize,
    pub(crate) distinct_owners: crate::fast_hash::PtrHashSet<usize>,
    pub(crate) samples: Vec<FromSpaceRef>,
}

const MAX_SAMPLES: usize = 32;

pub(super) fn fromspace_scan_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_FROMSPACE_SCAN").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

fn fromspace_scan_abort() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_GC_FROMSPACE_SCAN_ABORT").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

/// Is `space` a from-space for the copying minor that just ran? Eden always is;
/// so is whichever survivor semispace was active going INTO the cycle (the
/// flip happens later, inside `copying_reset_from_spaces_and_flip`).
#[inline]
fn is_from_space(space: crate::arena::HeapSpace) -> bool {
    space == crate::arena::HeapSpace::NurseryEden || space == crate::arena::active_survivor_space()
}

/// Scan one object's payload for from-space references.
///
/// # Safety
/// `header` must point at a live, walkable `GcHeader` whose `size` field covers
/// the allocation, which is what both the arena cursor and the malloc registry
/// guarantee.
unsafe fn scan_object(header: *mut GcHeader, report: &mut FromSpaceScanReport) {
    let total = (*header).size as usize;
    if total <= GC_HEADER_SIZE {
        return;
    }
    let user = (header as *mut u8).add(GC_HEADER_SIZE);
    let owner_space = crate::arena::classify_heap_space(user as usize);
    // A from-space object is about to be reclaimed; it may legitimately still
    // reference its dead peers. Only holders that SURVIVE the cycle matter.
    if is_from_space(owner_space) {
        return;
    }
    // ...and a FORWARDED owner is the same thing one step further on: it has
    // been superseded by its relocated copy, so it is dead by definition, and
    // its payload legitimately still names pre-move addresses. Old-gen defrag
    // and array growth both leave such stubs OUTSIDE from-space, where the
    // check above does not reach them, so every one of them was reported as an
    // offender. Measured on a Perry-compiled zod workload they were the single
    // largest population in the residue — all at `2^k - 1` element indices of
    // repeatedly-grown arrays (the last element written before each capacity
    // doubling), i.e. pure noise that buried the genuine holders underneath.
    //
    // COUNTED, not silently dropped: a filter that shrinks the offender count
    // without saying so reads exactly like progress, which is the failure mode
    // this instrument exists to prevent (#6942 / #7024).
    if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        report.forwarded_owners_skipped += 1;
        return;
    }
    report.objects_scanned += 1;

    let payload_words = (total - GC_HEADER_SIZE) / 8;
    let words = user as *const u64;
    for i in 0..payload_words {
        let bits = *words.add(i);
        report.words_scanned += 1;
        let Some(word) = super::root_words::decode_root_word(bits) else {
            continue;
        };
        let target = word.addr();
        let target_space = crate::arena::classify_heap_space(target);
        if !is_from_space(target_space) {
            continue;
        }
        // The target is in from-space. Did it move (missing rewrite) or was it
        // never evacuated (dangling)?
        let target_forwarded = if target >= GC_HEADER_SIZE {
            let th = (target - GC_HEADER_SIZE) as *const GcHeader;
            (*th).gc_flags & GC_FLAG_FORWARDED != 0
        } else {
            false
        };
        if target_forwarded {
            report.missing_rewrites += 1;
        } else {
            report.dangling += 1;
        }
        report.distinct_owners.insert(header as usize);
        let slot_addr = words.add(i) as usize;
        let dirty_now = super::barrier::dirty_now_for_addr(slot_addr);
        let ever_dirty = super::barrier::ever_dirty_for_addr(slot_addr);
        if SNAPSHOT_PAGES.with(|c| {
            c.borrow()
                .contains(&crate::arena::generation_page_for_addr(slot_addr))
        }) {
            report.in_snapshot += 1;
        } else {
            report.not_in_snapshot += 1;
        }
        if !ever_dirty {
            report.never_dirty += 1;
        } else if !dirty_now {
            report.lost_dirty += 1;
        } else {
            report.dirty_but_missed += 1;
        }
        if report.samples.len() < MAX_SAMPLES {
            report.samples.push(FromSpaceRef {
                owner_header: header as usize,
                owner_obj_type: (*header).obj_type,
                owner_space,
                slot_offset: i * 8,
                target,
                target_space,
                target_forwarded,
                nanboxed: matches!(word, super::root_words::RootWord::Nanboxed { .. }),
                slot_dirty_now: super::barrier::dirty_now_for_addr(words.add(i) as usize),
                slot_ever_dirty: super::barrier::ever_dirty_for_addr(words.add(i) as usize),
                owner_flags: (*header).gc_flags,
            });
        }
        if fromspace_scan_abort() {
            report_and_abort(report);
        }
    }
}

/// Walk the whole heap and report every surviving reference into from-space.
pub(crate) fn scan_heap_for_fromspace_refs() -> FromSpaceScanReport {
    let mut report = FromSpaceScanReport::default();

    // --- arena census (Eden / survivors / old / longlived) ------------------
    let mut builder =
        crate::arena::ArenaObjectCursorBuilder::new(crate::arena::ArenaWalkOrder::Address);
    let mut cursor = loop {
        let mut budget = usize::MAX;
        if let Some(cursor) = builder.step(&mut budget) {
            break cursor;
        }
    };
    loop {
        let mut budget = usize::MAX;
        match cursor.next_budgeted(&mut budget) {
            Some((header_ptr, _block_idx)) => unsafe {
                scan_object(header_ptr as *mut GcHeader, &mut report);
            },
            None => {
                if cursor.is_finished() {
                    break;
                }
            }
        }
    }

    // --- malloc-tracked objects --------------------------------------------
    let malloc_headers: Vec<*mut GcHeader> =
        MALLOC_STATE.with(|s| s.borrow().objects.iter().copied().collect());
    for header in malloc_headers {
        if header.is_null() {
            continue;
        }
        unsafe {
            scan_object(header, &mut report);
        }
    }

    report
}

fn describe(r: &FromSpaceRef) -> String {
    format!(
        "  owner={:#x} type={} space={:?} +{} {} -> {:#x} ({:?}) {} [slot dirty_now={} ever_dirty={} owner_flags={:#x} marked={}]",
        r.owner_header,
        r.owner_obj_type,
        r.owner_space,
        r.slot_offset,
        if r.nanboxed { "nanbox" } else { "bare" },
        r.target,
        r.target_space,
        if r.target_forwarded {
            "MISSING-REWRITE (target moved)"
        } else {
            "DANGLING (target not evacuated)"
        },
        r.slot_dirty_now,
        r.slot_ever_dirty,
        r.owner_flags,
        r.owner_flags & GC_FLAG_MARKED != 0
    )
}

fn report_and_abort(report: &FromSpaceScanReport) -> ! {
    emit_report(report, "abort");
    panic!(
        "gc from-space scan: {} missing rewrite(s), {} dangling reference(s) survived the rewrite pass",
        report.missing_rewrites, report.dangling
    );
}

pub(super) fn emit_report(report: &FromSpaceScanReport, phase: &str) {
    eprintln!(
        "[gc-fromspace-scan {}] objects={} words={} fwd_owners_skipped={} missing_rewrites={} dangling={} owners={} | never_dirty={} lost_dirty={} dirty_but_missed={}",
        phase,
        report.objects_scanned,
        report.words_scanned,
        report.forwarded_owners_skipped,
        report.missing_rewrites,
        report.dangling,
        report.distinct_owners.len(),
        report.never_dirty,
        report.lost_dirty,
        report.dirty_but_missed
    );
    eprintln!(
        "[gc-fromspace-scan {}]   in_snapshot={} not_in_snapshot={}",
        phase, report.in_snapshot, report.not_in_snapshot
    );
    for sample in &report.samples {
        eprintln!("{}", describe(sample));
    }
}

/// Entry point called from the copying minor, after the rewrite pass and before
/// `copying_reset_from_spaces_and_flip`.
pub(super) fn run_fromspace_scan(snapshot: &super::RememberedDirtySnapshot) {
    if !fromspace_scan_enabled() {
        return;
    }
    SNAPSHOT_PAGES.with(|c| {
        *c.borrow_mut() = snapshot.dirty_old_pages.iter().copied().collect();
    });
    let report = scan_heap_for_fromspace_refs();
    if report.missing_rewrites > 0 || report.dangling > 0 {
        emit_report(&report, "OFFENDERS");
    } else {
        emit_report(&report, "clean");
    }
}

thread_local! {
    static SNAPSHOT_PAGES: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_set());
}
