//! Honest-pointer heap invariant, checked under `PERRY_GC_VERIFY_MARK` in
//! contract-check builds (`PERRY_CHECK_CONTRACTS=1`) and unit tests; release
//! and ordinary instrumented builds carry none of it. Every `POINTER_TAG` word a
//! collection scans names a GC-headed object, either on this thread's heap or
//! in process-wide immortal space (image strings, leaked symbol cells).
//! Native registry ids travel under `HANDLE_TAG`, so an id-band payload under
//! `POINTER_TAG` is a producer bug, as is any payload without a GC header.
//! A live region of another thread is counted separately: zero-copy transfer
//! cells are the one cross-heap case, and ownership is the collector's
//! question there.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HonestTarget {
    CurrentHeap,
    Immortal,
    OtherHeap,
}

#[derive(Clone, Debug, Default)]
pub(super) struct HonestPointerStats {
    pub(super) objects: usize,
    pub(super) words: usize,
    pub(super) current: usize,
    pub(super) immortal: usize,
    pub(super) other_heap: usize,
    pub(super) violations: usize,
    pub(super) first: Option<(usize, u64, &'static str)>,
}

impl HonestPointerStats {
    fn record(&mut self, parent: usize, bits: u64) {
        self.words += 1;
        match classify_pointer_word(bits) {
            Ok(HonestTarget::CurrentHeap) => self.current += 1,
            Ok(HonestTarget::Immortal) => self.immortal += 1,
            Ok(HonestTarget::OtherHeap) => self.other_heap += 1,
            Err(why) => {
                self.violations += 1;
                if self.first.is_none() {
                    self.first = Some((parent, bits, why));
                }
            }
        }
    }
}

/// Classify one `POINTER_TAG` word without trusting its payload: the region
/// descriptor or an exact allocator/image/symbol lookup must vouch for the
/// address before its header is read.
#[inline(never)]
pub(super) fn classify_pointer_word(bits: u64) -> Result<HonestTarget, &'static str> {
    debug_assert_eq!(bits & TAG_MASK, POINTER_TAG);
    let addr = (bits & POINTER_MASK) as usize;
    if crate::value::addr_class::is_handle_band(addr) {
        return Err("id-band payload under POINTER_TAG");
    }
    if addr % std::mem::align_of::<GcHeader>() != 0 {
        return Err("misaligned payload");
    }
    let header = addr.wrapping_sub(GC_HEADER_SIZE) as *const GcHeader;
    match crate::arena::classify_region_ownership(addr) {
        crate::arena::RegionOwnership::Current => unsafe {
            if gc_type_info((*header).obj_type).is_none()
                || ((*header).size as usize) < GC_HEADER_SIZE
            {
                return Err("malformed arena header");
            }
            Ok(HonestTarget::CurrentHeap)
        },
        crate::arena::RegionOwnership::OtherHeap => Ok(HonestTarget::OtherHeap),
        crate::arena::RegionOwnership::Outside => {
            if super::malloc::gc_malloc_header_is_local(header) {
                return Ok(HonestTarget::CurrentHeap);
            }
            if unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.is_some()
                || crate::symbol::is_registered_symbol(addr)
            {
                return Ok(HonestTarget::Immortal);
            }
            Err("no GC header on this heap or in immortal space")
        }
    }
}

/// Walk the traced slots of every object a collection must keep valid and
/// classify each `POINTER_TAG` word. Called once marks are final and before
/// anything is reclaimed: marked or pinned objects, and every old or tenured
/// object (an old parent keeps its young children alive through the
/// remembered set). Unmarked young garbage is skipped. Aborts on a violation.
/// Out of line: a diagnostic must not grow the collector's step functions.
#[cold]
#[inline(never)]
pub(super) fn verify_honest_pointers(phase: &str) -> HonestPointerStats {
    let mut stats = HonestPointerStats::default();
    let mut visit = |header: *mut GcHeader| unsafe {
        if header.is_null() {
            return;
        }
        let flags = (*header).gc_flags;
        if flags & GC_FLAG_FORWARDED != 0 {
            return;
        }
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        let live = flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0
            || if flags & GC_FLAG_ARENA != 0 {
                !matches!(
                    crate::arena::classify_heap_generation(user),
                    crate::arena::HeapGeneration::Nursery
                )
            } else {
                flags & GC_FLAG_TENURED != 0
            };
        if !live {
            return;
        }
        stats.objects += 1;
        let parent = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        visit_gc_rewrite_slots(header, |slot| {
            let bits = slot.read();
            if bits & TAG_MASK == POINTER_TAG {
                stats.record(parent, bits);
            }
        });
    };
    crate::arena::arena_walk_objects(|hp| visit(hp as *mut GcHeader));
    for header in super::verify::malloc_headers_for_verification() {
        visit(header);
    }
    // WRT: realm image cells are live by definition (born MARKED|PINNED).
    super::image_cells::for_each_image_cell(|header| visit(header));
    report_honest_pointers(phase, &stats);
    stats
}

/// Root words reach the marker one at a time. Precise-root admission already
/// rejects a header-less root under verification; this adds the id-band rule.
/// Out of line: it sits on the marker's rare id-band arm.
#[cold]
#[inline(never)]
pub(super) fn verify_honest_root_word(bits: u64) {
    if bits & TAG_MASK != POINTER_TAG {
        return;
    }
    if crate::value::addr_class::is_handle_band((bits & POINTER_MASK) as usize) {
        panic!("[gc-honest-verify:root] id-band payload under POINTER_TAG: word {bits:#x}");
    }
}

fn report_honest_pointers(phase: &str, stats: &HonestPointerStats) {
    if let Some((parent, bits, why)) = stats.first {
        panic!(
            "[gc-honest-verify:{phase}] violations={} words={} objects={} | first parent={parent:#x} word={bits:#x}: {why}",
            stats.violations, stats.words, stats.objects
        );
    }
    eprintln!(
        "[gc-honest-verify:{phase}] OK objects={} words={} current={} immortal={} other_heap={}",
        stats.objects, stats.words, stats.current, stats.immortal, stats.other_heap
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Negative controls: the classifier must reject the words a dishonest
    /// producer would write, or a green verification run proves nothing.
    #[test]
    fn classifier_rejects_id_band_misaligned_and_headerless_words() {
        let object = crate::object::js_object_alloc(0, 0) as u64;
        assert_eq!(
            classify_pointer_word(POINTER_TAG | object),
            Ok(HonestTarget::CurrentHeap)
        );
        let id = crate::value::addr_class::FETCH_HANDLE_BAND_START as u64;
        assert!(classify_pointer_word(POINTER_TAG | id).is_err());
        assert!(classify_pointer_word(POINTER_TAG | (object + 1)).is_err());
        static NOT_A_CELL: [u64; 4] = [0; 4];
        let headerless = std::ptr::addr_of!(NOT_A_CELL[2]) as u64;
        assert!(classify_pointer_word(POINTER_TAG | headerless).is_err());
    }

    #[test]
    #[should_panic(expected = "id-band payload under POINTER_TAG")]
    fn root_check_aborts_on_an_id_band_pointer_word() {
        verify_honest_root_word(
            POINTER_TAG | crate::value::addr_class::FETCH_HANDLE_BAND_START as u64,
        );
    }
}
