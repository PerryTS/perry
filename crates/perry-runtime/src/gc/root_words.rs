//! Root decoding is selected by the source owner, then shared by marking and
//! relocation. Generated global/statepoint slots hold JSValue words at rest;
//! runtime pointer fields supply a GcPointer. Only legacy shadow frames (including
//! WASI) and ambiguous heap words retain the conservative mixed-word decoder.
//! A numeric JSValue whose bits happen to equal a heap address is never a root.
//!
//! Precise roots keep their producer-declared encoding. Before touching a
//! header, marking checks current-heap ownership using live arena and malloc
//! metadata; process-global providers can also emit another agent's roots.
//! This ownership check does not infer a word's type or use a census snapshot.

use super::*;

/// Exclusive floor of the NaN-tag space. Every bit pattern at or above this
/// is a NaN-boxed immediate (number, boolean, `undefined`/`null`, SSO
/// string, INT32, handle) or one of the three heap tags — never a bare
/// address.
const NAN_TAG_FLOOR: u64 = 0x7FF8_0000_0000_0000;

/// Inclusive floor for a bare heap address. Below the first page nothing is
/// mappable, so a smaller word is a small integer or null, not a pointer.
const BARE_ADDR_MIN: u64 = 0x1000;

/// A word that decoded to a heap reference, in the form it was stored in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RootWord {
    /// NaN-boxed reference. `tag` is `POINTER_TAG`, `STRING_TAG` or
    /// `BIGINT_TAG` and must be re-applied when the address is rewritten.
    Nanboxed { addr: usize, tag: u64 },
    /// Bare (untagged) user address stored directly in the word.
    Bare { addr: usize },
}

impl RootWord {
    /// The user address the collector must mark / relocate.
    #[inline]
    pub(super) fn addr(self) -> usize {
        match self {
            Self::Nanboxed { addr, .. } | Self::Bare { addr } => addr,
        }
    }

    /// Re-encode `new_addr` in this word's original form, so a rewrite
    /// preserves tagged-ness rather than converting between the two.
    #[inline]
    pub(super) fn encode(self, new_addr: usize) -> u64 {
        match self {
            Self::Nanboxed { tag, .. } => tag | (new_addr as u64 & POINTER_MASK),
            Self::Bare { .. } => new_addr as u64,
        }
    }
}

/// Decode a root-slot / heap word into the heap reference it carries, if any.
///
/// This is the one predicate behind [`mark_mutable_root_bits`],
/// `try_rewrite_value` and `heap_word_candidate_addr`; see the module docs
/// for why they must not diverge.
#[inline]
pub(super) fn decode_root_word(bits: u64) -> Option<RootWord> {
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        let payload = bits & POINTER_MASK;
        // arm64_32 (ILP32 — watchOS): a real heap pointer fits in 32 bits, so
        // `payload as usize` is lossless only when the high 16 payload bits
        // are zero. A mistagged / immediate value with a >32-bit payload
        // would otherwise truncate to a garbage 32-bit address that the GC
        // marks, derefs, or rewrites — corrupting unrelated heap memory.
        #[cfg(not(target_pointer_width = "64"))]
        if payload > 0xFFFF_FFFF {
            return None;
        }
        let addr = payload as usize;
        return (addr != 0).then_some(RootWord::Nanboxed { addr, tag });
    }
    // Not a heap tag. Everything else in the NaN-tag space is an immediate.
    if tag >= NAN_TAG_FLOOR {
        return None;
    }
    // Bare address: must fit the 48-bit user-address range. Plain `f64`
    // payloads and raw integers land outside it and are rejected here.
    if !(BARE_ADDR_MIN..=POINTER_MASK).contains(&bits) {
        return None;
    }
    // Same ILP32 guard for the bare form.
    #[cfg(not(target_pointer_width = "64"))]
    if bits > 0xFFFF_FFFF {
        return None;
    }
    Some(RootWord::Bare {
        addr: bits as usize,
    })
}

/// Legacy untyped shadow-frame marking. These frames have no type descriptor
/// and may contain bare pointers as well as JSValues, so retain validation.
#[inline]
pub(super) fn mark_mutable_root_bits(bits: u64, valid_ptrs: &ValidPointerSet) {
    if super::full_trace::handle_trace_active()
        && super::full_trace::observe_handle(bits, valid_ptrs)
    {
        return;
    }
    let Some(word) = decode_root_word(bits) else {
        return;
    };
    try_mark_raw_root_addr(word.addr(), valid_ptrs);
}

/// Encoding supplied by the owner of a precise root. This is transient visitor
/// data, never a registry of slot kinds. Generated mutable slots use JSValue
/// encoding at rest; native owners with pointer fields provide GcPointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreciseRoot {
    JSValue(u64),
    GcPointer(usize),
}

impl PreciseRoot {
    #[inline]
    fn address(self) -> Option<usize> {
        match self {
            Self::JSValue(bits) => decode_tagged_root_word(bits).map(RootWord::addr),
            Self::GcPointer(addr) => (addr != 0).then_some(addr),
        }
    }

    #[inline]
    fn bits(self) -> u64 {
        match self {
            Self::JSValue(bits) => bits,
            Self::GcPointer(addr) => POINTER_TAG | (addr as u64 & POINTER_MASK),
        }
    }
}

/// A decoded root may belong to another collector when its provider is
/// process-global. Consult live ownership metadata before any header access,
/// including for post-census births and generations outside the census.
#[inline]
pub(super) fn owns_precise_root_addr(addr: usize) -> bool {
    if crate::arena::classify_heap_generation(addr) != crate::arena::HeapGeneration::Unknown {
        return true;
    }
    // An unowned candidate need not point into an allocation, so compute the
    // candidate address without in-bounds pointer arithmetic or dereferencing it.
    let header = addr.wrapping_sub(GC_HEADER_SIZE) as *const GcHeader;
    super::malloc::gc_malloc_header_is_owned(header)
}

/// Mark a root whose producer supplied its representation. The object's own
/// header is authoritative after confirming that this collector owns it.
///
/// Invalid local typed roots are producer bugs. Instrumented verification and
/// unit tests validate the header before changing its color. The evacuation
/// verifier checks that root slots do not retain moved addresses.
#[inline]
pub(crate) fn mark_precise_root(root: PreciseRoot, valid_ptrs: &ValidPointerSet) -> bool {
    mark_precise_root_in_scope(root, valid_ptrs, None)
}

/// Root writes share the marker, with the incremental cycle's scope: a minor
/// shades nursery objects only and a forwarding alias needs no new shading.
#[inline]
pub(super) fn mark_precise_root_in_scope(
    root: PreciseRoot,
    valid_ptrs: &ValidPointerSet,
    write_scope: Option<bool>,
) -> bool {
    let Some(addr) = root.address() else {
        return false;
    };
    // A tagged handle is a root of its provider's JSValue edges, not a GC
    // allocation. Decode the source first so numeric bits cannot observe ids.
    if crate::value::addr_class::is_handle_band(addr) {
        if super::full_trace::handle_trace_active() {
            super::full_trace::observe_handle(root.bits(), valid_ptrs);
        }
        return false;
    }
    if !owns_precise_root_addr(addr) {
        return false;
    }
    unsafe {
        let header = header_from_user_ptr(addr as *const u8);
        let flags = (*header).gc_flags;
        // This is a structural header check, not snapshot membership:
        // owners may publish births after the census or in generations the
        // current collection does not enumerate. Evacuation verification
        // separately rejects slots left pointing at moved objects.
        #[cfg(test)]
        assert!(
            gc_type_info((*header).obj_type).is_some() && (*header).size as usize >= GC_HEADER_SIZE,
            "invalid precise root header: {addr:#x}"
        );
        #[cfg(all(not(test), perry_gc_instruments))]
        if super::gc_verify_mark_enabled() {
            assert!(
                gc_type_info((*header).obj_type).is_some()
                    && (*header).size as usize >= GC_HEADER_SIZE,
                "invalid precise root header: {addr:#x}"
            );
        }
        if let Some(nursery_only) = write_scope {
            if flags & GC_FLAG_FORWARDED != 0
                || (nursery_only && !crate::arena::pointer_in_nursery(addr))
            {
                return false;
            }
        }
        if flags & GC_FLAG_MARKED != 0 || super::pin::pinned_counts_as_marked(flags) {
            return false;
        }
        (*header).gc_flags = flags | GC_FLAG_MARKED;
        push_mark_seed(header);
        true
    }
}

/// Decode a JSValue root, never accepting a numeric bit pattern as an address.
#[inline]
pub(super) fn decode_nanboxed_root_word(bits: u64) -> Option<RootWord> {
    let word = decode_tagged_root_word(bits)?;
    (!crate::value::addr_class::is_handle_band(word.addr())).then_some(word)
}

/// Decode the pointer-bearing tags, including native handle payloads. Marking
/// dispatches handles; relocation leaves them alone. Both reject numeric bits.
#[inline]
fn decode_tagged_root_word(bits: u64) -> Option<RootWord> {
    match bits & TAG_MASK {
        POINTER_TAG | STRING_TAG | BIGINT_TAG => {
            let payload = bits & POINTER_MASK;
            #[cfg(not(target_pointer_width = "64"))]
            if payload > usize::MAX as u64 {
                return None;
            }
            let addr = payload as usize;
            (addr != 0).then_some(RootWord::Nanboxed {
                addr,
                tag: bits & TAG_MASK,
            })
        }
        _ => None,
    }
}
