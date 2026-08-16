//! Validation for the *target* of a forwarding pointer (#8174).
//!
//! `GC_FLAG_FORWARDED` says "the first payload word of this object is the
//! address it moved to". Both forwarding walkers —
//! [`CopyingNurseryCollector::rewrite_raw_addr`](super::copying) and
//! [`try_rewrite_raw_addr`](super::verify) — used to trust that word
//! unconditionally: whatever it held became the answer, and if it did not
//! classify as heap the walk simply *stopped there* and returned it.
//!
//! That is safe for a slot the collector already proved is a live reference,
//! and it is not safe for a **metadata key** (`RuntimeRootVisitor::
//! visit_metadata_*`), which is deliberately NOT a root: its object may have
//! died, the arena may have recycled the address, and the recycled payload
//! bytes then get read as a `GcHeader`. #8040 hit exactly that — recycled
//! bytes with `gc_flags = 0x86` (`GC_FLAG_FORWARDED` set), `obj_type = 104`,
//! whose "forwarding pointer" was a NaN-boxed value (`0x7FFF…`). The walk
//! stopped after one hop and returned it; the caller masked it to 48 bits and
//! got a live, unrelated survivor. The class id of a synthetic class was then
//! bound to an interned string, and the program failed several collections
//! later with `TypeError: value is not a function`.
//!
//! #8168 removed the one dead key that reached this. This module removes the
//! *following*: a forwarding target that is not the start of a heap object is
//! refused outright rather than returned as an address.

use super::*;

crate::perry_thread_local! {
    /// Forwarding walks refused because the address being walked did not read
    /// back as a real arena object header.
    static REFUSED_FORWARDING_SOURCES: Cell<u64> = const { Cell::new(0) };
    /// Forwarding walks refused because the *target* word was not an object
    /// start.
    static REFUSED_FORWARDING_TARGETS: Cell<u64> = const { Cell::new(0) };
}

/// Running count of addresses this thread declined to read a forwarding
/// header out of. Non-zero means a stale key reached a forwarding walk — the
/// shape of #8040 — so a test that plants one can assert it moved, and a fix
/// can assert it went back to zero.
pub(crate) fn refused_forwarding_source_count() -> u64 {
    REFUSED_FORWARDING_SOURCES.with(Cell::get)
}

/// Running count of forwarding targets this thread declined to follow.
pub(crate) fn refused_forwarding_target_count() -> u64 {
    REFUSED_FORWARDING_TARGETS.with(Cell::get)
}

crate::perry_thread_local! {
    static REPORTED_SOURCES: Cell<u64> = const { Cell::new(0) };
    static REPORTED_TARGETS: Cell<u64> = const { Cell::new(0) };
}

/// Report the refusals since the last call, and only when there were any.
///
/// A healthy run prints nothing at all, so this cannot perturb a log any gate
/// parses. A line here is a POSITIVE report that a stale or corrupt
/// `GC_FLAG_FORWARDED` header reached a rewrite walk — the #8040 signal, which
/// until now was invisible until it corrupted something several collections
/// later in an unrelated function.
pub(super) fn report_forwarding_refusals(phase: &str) {
    if !crate::gc::gc_diag_enabled() {
        return;
    }
    let sources = refused_forwarding_source_count();
    let targets = refused_forwarding_target_count();
    let new_sources = sources - REPORTED_SOURCES.with(Cell::get);
    let new_targets = targets - REPORTED_TARGETS.with(Cell::get);
    REPORTED_SOURCES.with(|c| c.set(sources));
    REPORTED_TARGETS.with(|c| c.set(targets));
    if new_sources == 0 && new_targets == 0 {
        return;
    }
    eprintln!(
        "[gc-forwarding] {phase} refused_sources={new_sources}          refused_targets={new_targets} total_sources={sources}          total_targets={targets}"
    );
}

#[cold]
fn note_refused_forwarding_source() {
    REFUSED_FORWARDING_SOURCES.with(|c| c.set(c.get().saturating_add(1)));
}

#[cold]
fn note_refused_forwarding_target() {
    REFUSED_FORWARDING_TARGETS.with(|c| c.set(c.get().saturating_add(1)));
}

/// The header to read a forwarding pointer out of, or `None` to stop the walk.
///
/// The previous gate was `classify_heap_space(addr - 8) != Unknown`, i.e.
/// "could this address be in the heap". That admits any recycled byte in the
/// arena, and #8040's recycled bytes duly presented `gc_flags = 0x86` with
/// `obj_type = 104` — a type id no `GcTypeInfo` entry exists for.
/// [`plausible_gc_header`] rejects exactly that, and every real forwarding
/// source passes it: `set_forwarding_address` overwrites the first payload
/// word and ORs one flag bit, leaving `obj_type`, `size` and `GC_FLAG_ARENA`
/// intact, and the only production installers of a forwarding pointer
/// (`copying::move_young`, promotion, `gc::oldgen` defrag, and
/// `array::push_pop`'s growth stub, which requires `GC_FLAG_ARENA` at install
/// time) all operate on arena objects.
///
/// This is NOT the `self.ptrs.classify()` gate that
/// `CopyingNurseryCollector::rewrite_raw_addr` documents as having broken
/// `shapes.entries`. That one additionally narrows on SPACE and resolves the
/// active/inactive survivor thread-locals, which is what rejected legitimate
/// from-space keys. The header test carries none of that.
pub(super) fn forwarding_walk_header(user_addr: usize) -> Option<*mut GcHeader> {
    if user_addr < GC_HEADER_SIZE {
        return None;
    }
    // #7742: one page-map probe answers both classifications. The range base
    // is the guard that keeps a garbage candidate at the very start of a
    // registered range from becoming a read of the unmapped page below it.
    let (_space, range_base) = crate::arena::classify_heap_space_in_range(user_addr)?;
    let header_addr = user_addr - GC_HEADER_SIZE;
    if header_addr < range_base {
        return None;
    }
    let header = header_addr as *mut GcHeader;
    if !unsafe { plausible_gc_header(header, true) } {
        note_refused_forwarding_source();
        return None;
    }
    Some(header)
}

/// Is `user_addr` a plausible destination for a forwarding pointer — the
/// start of a heap object, rather than a word that merely happens to sit
/// where a forwarding pointer would?
///
/// Two arms, because the two kinds of legitimate target admit different
/// evidence:
///
/// * **On a registered arena range** the header is mapped by construction, so
///   the strongest test available applies: it must read back as a real arena
///   object header ([`plausible_gc_header`] — registered `obj_type`, sane
///   size, `GC_FLAG_ARENA`). Every GC-installed forwarding pointer (evacuation
///   in `copying::move_young`, promotion, old-gen defrag in `gc::oldgen`)
///   lands here.
/// * **Off-arena**, the one legitimate target is a malloc'd allocation: an
///   array-growth stub whose new head outgrew the arena
///   (`array::push_pop::install_array_growth_forwarding_with`, #233). The
///   malloc registry cannot be consulted from here without forcing the
///   registry build that `gc::dead_owner`'s `PostTraceProbe` documents as a
///   state transition the fallback mark-sweep path must not make, so this arm
///   keeps the magnitude test and no more. That is still strictly more than
///   the previous code did: `is_plausible_heap_addr` rejects the handle band
///   and everything at or above `HEAP_MAX` (`0x8000_0000_0000`), which is what
///   makes the NaN-boxed `0x7FFF…` word from #8040 fail here.
pub(super) fn forwarding_target_is_object_start(user_addr: usize) -> bool {
    if user_addr < GC_HEADER_SIZE {
        return false;
    }
    if let Some((_space, range_base)) = crate::arena::classify_heap_space_in_range(user_addr) {
        let header_addr = user_addr - GC_HEADER_SIZE;
        return header_addr >= range_base
            && unsafe { plausible_gc_header(header_addr as *mut GcHeader, true) };
    }
    crate::value::addr_class::is_plausible_heap_addr(user_addr)
}

/// [`forwarding_target_is_object_start`] plus the refusal census. Call this
/// from a forwarding walk; `false` means "do not follow, and do not return
/// this address either".
#[inline]
pub(super) fn accept_forwarding_target(user_addr: usize) -> bool {
    if forwarding_target_is_object_start(user_addr) {
        return true;
    }
    note_refused_forwarding_target();
    false
}
