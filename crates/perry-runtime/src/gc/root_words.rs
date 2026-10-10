//! Root decoding is selected by the source owner, then shared by marking and
//! relocation. Generated global/statepoint slots hold JSValue words at rest;
//! runtime pointer fields supply a GcPointer. Only legacy shadow frames (including
//! WASI) and ambiguous heap words retain the conservative mixed-word decoder.
//! A numeric JSValue whose bits happen to equal a heap address is never a root.
//!
//! Precise roots keep their producer-declared encoding. Before touching a
//! header, admission asks the arena region descriptor who owns the address
//! (one descriptor read). Outside every arena region the object's own header
//! is the authority: an ARENA header there is not this heap's object, and any
//! other header is a malloc or process-lifetime cell of this heap. Admission
//! never infers a word's type, consults a census snapshot or walks a registry.

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

/// Verification of the precise-root contract: always on in unit tests (a test
/// can turn it off to observe the release path), and behind
/// `PERRY_GC_VERIFY_MARK` in instrumented builds.
#[inline]
pub(super) fn precise_root_verification() -> bool {
    #[cfg(test)]
    {
        PRECISE_ROOT_VERIFICATION.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        super::gc_verify_mark_enabled()
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static PRECISE_ROOT_VERIFICATION: std::cell::Cell<bool> =
        const { std::cell::Cell::new(true) };
}

/// The JSValue-root contract (#12271): with conservative scanning off, a
/// global, temp, handle or statepoint root declared as a JSValue holds a tagged
/// JSValue; a raw pointer needs a declared raw kind. An untagged word naming
/// one of this heap's objects is a producer that stored a bare address, so
/// the object is unrooted. Checked under verification when the slot is
/// registered and whenever a collector reads it. Address-shaped numbers that
/// name no object of this heap (denormal doubles) stay numbers.
#[inline]
pub(super) fn verify_jsvalue_root_word(bits: u64, source: &str) {
    // Only an address-shaped word can name an object; test that before the
    // verification switch so a numeric root word costs one range compare.
    if (BARE_ADDR_MIN..=POINTER_MASK).contains(&bits) && precise_root_verification() {
        verify_bare_address_root_word(bits as usize, source);
    }
}

#[cold]
#[inline(never)]
fn verify_bare_address_root_word(addr: usize, source: &str) {
    let names_object = match crate::arena::classify_region_ownership(addr) {
        crate::arena::RegionOwnership::Current => true,
        crate::arena::RegionOwnership::OtherHeap => false,
        crate::arena::RegionOwnership::Outside => {
            addr % 8 == 0
                && super::malloc::gc_malloc_header_is_local(
                    addr.wrapping_sub(GC_HEADER_SIZE) as *const GcHeader
                )
        }
    };
    assert!(
        !names_object,
        "untagged heap address {addr:#x} in a JSValue {source} root: the producer must store a tagged JSValue or declare a raw root"
    );
}

/// A root that is neither this heap's object nor provably another live heap's
/// is a producer bug: a mis-typed, stale or foreign word. Verification aborts
/// before any header write; release leaves the header untouched.
#[cold]
#[inline(never)]
fn invalid_precise_root(addr: usize, why: &str) -> Option<*mut GcHeader> {
    if precise_root_verification() {
        #[cfg(target_os = "linux")]
        let region = crate::arena::region_classify(addr).map(|r| {
            let this_thread = r.thread == crate::tls_hot::thread_identity();
            (r.kind, r.space, this_thread, r.base, r.end)
        });
        #[cfg(not(target_os = "linux"))]
        let region: Option<()> = None;
        panic!(
            "invalid precise root header: {addr:#x} ({why}); region (kind, space, this thread, base, end): {region:?}"
        );
    }
    None
}

/// Admit a decoded precise root before any header write: the header this
/// collector may mark, pin or relocate, or `None`.
///
/// O(1). The arena region descriptor answers first. A live region of another
/// thread is that thread's heap (a Worker's, or a zero-copy transferred cell),
/// so its root is dropped silently and its header is never read. Outside every arena region the header decides: an ARENA
/// header there is a producer bug; any other header is a malloc or
/// process-lifetime cell of this heap. Verification also checks the header's
/// structure, and that an unmarked header outside the arena is one of this
/// agent's malloc allocations.
#[inline]
pub(super) fn admit_precise_root_addr(addr: usize) -> Option<*mut GcHeader> {
    let outside = match crate::arena::classify_region_ownership(addr) {
        crate::arena::RegionOwnership::Current => false,
        crate::arena::RegionOwnership::OtherHeap => return None,
        crate::arena::RegionOwnership::Outside => true,
    };
    unsafe {
        let header = header_from_user_ptr(addr as *const u8);
        let flags = (*header).gc_flags;
        if outside && flags & GC_FLAG_ARENA != 0 {
            return invalid_precise_root(addr, "arena header outside this heap's regions");
        }
        if precise_root_verification() {
            if !(gc_type_info((*header).obj_type).is_some()
                && (*header).size as usize >= GC_HEADER_SIZE)
            {
                return invalid_precise_root(addr, "malformed header");
            }
            // A marked header needs no write. An unmarked one outside the
            // arena must be this heap's own malloc allocation.
            if outside
                && flags & GC_FLAG_MARKED == 0
                && !super::malloc::gc_malloc_header_is_local(header)
            {
                return invalid_precise_root(addr, "not this heap's allocation");
            }
        }
        Some(header)
    }
}

/// Mark a root whose producer supplied its representation. After admission
/// the object's own header is authoritative.
///
/// Invalid typed roots are producer bugs; admission reports them under
/// verification. The evacuation verifier checks that root slots do not retain
/// moved addresses.
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
        if let PreciseRoot::JSValue(bits) = root {
            verify_jsvalue_root_word(bits, "scanned");
        }
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
    // A minor shades only this heap's nursery. That region test is itself an
    // ownership proof, so a root write outside it costs no further lookup.
    if write_scope == Some(true) && !crate::arena::pointer_in_nursery(addr) {
        return false;
    }
    let Some(header) = admit_precise_root_addr(addr) else {
        return false;
    };
    unsafe { mark_admitted_root_header(header, write_scope.is_some()) }
}

/// Shade an admitted header. A root write never re-shades a forwarding alias.
#[inline]
pub(super) unsafe fn mark_admitted_root_header(header: *mut GcHeader, root_write: bool) -> bool {
    let flags = (*header).gc_flags;
    if root_write && flags & GC_FLAG_FORWARDED != 0 {
        return false;
    }
    if flags & GC_FLAG_MARKED != 0 || super::pin::pinned_counts_as_marked(flags) {
        return false;
    }
    (*header).gc_flags = flags | GC_FLAG_MARKED;
    push_mark_seed(header);
    true
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
