//! The single decoder shared by the collector's *mark* and *rewrite* paths
//! for words that may hold a heap reference (#6910).
//!
//! # The invariant
//!
//! **Anything the rewrite path will relocate, the mark path must mark.**
//!
//! A GC root slot or heap word can carry a heap reference in two forms:
//!
//! - **NaN-boxed** — `POINTER_TAG` / `STRING_TAG` / `BIGINT_TAG` in the top
//!   16 bits, user address in the low 48. This is what all shipped codegen
//!   stores in a shadow-stack slot ("tagged at rest").
//! - **Bare** — the untagged user address written straight into the word.
//!   Module-variable globals have always done this, runtime side tables that
//!   hold `*mut T` do it, and the representation-selection RFC's unboxed
//!   pointer reps (§5.6 `Ptr<Shape>`, `Str`, closure envs) are designed to.
//!
//! Before #6910 the two forms were decoded by four hand-copied predicates
//! that did not agree:
//!
//! | surface | accepted |
//! |---|---|
//! | `try_mark_value` (fed by shadow-stack root marking) | NaN-boxed only |
//! | `try_rewrite_value` (mutable-root + heap-slot rewrite) | NaN-boxed **and bare** |
//! | `CopyingPointerSet::decode_bits` (copying minor) | NaN-boxed **and bare** |
//! | `heap_word_candidate_addr` (incremental mark barrier) | NaN-boxed **and bare** |
//!
//! So a bare address in a shadow-stack slot was accepted by the rewrite,
//! copying and barrier paths but **invisible to mark-sweep root marking**.
//! If such a slot were the only reference to an object, the mark phase would
//! skip it, the sweep would free it, and the slot would be left pointing at
//! reclaimed memory — a use-after-free, not merely a missed rewrite (an
//! unmarked object is never evacuated, so it never gets a forwarding address
//! for the rewrite pass to follow).
//!
//! The failure would also have been invisible to the test suite: the unit
//! tests default to `ConservativeStackScanMode::Full`
//! (`gc::roots::conservative_stack_scan_mode`), whose native-stack scan
//! accepts bare addresses and would have rescued the object, while
//! production resolves `Auto -> SkipDisabled` and has no such safety net.
//! Green tests, prod-only UAF. `gc::tests::root_words` therefore pins the
//! invariant with the conservative scan explicitly `Disabled`.
//!
//! # Using this module
//!
//! Decode through [`decode_root_word`] and act on the result; do not re-type
//! the tag comparisons or the address-range literals at a call site. Adding a
//! representation here teaches the mark path, the rewrite path and the
//! incremental barrier at once, which is the property that keeps them from
//! drifting apart again.
//!
//! Two collector surfaces deliberately do **not** route through here, and
//! both are safe because they are *supersets* of this decoder rather than
//! subsets:
//!
//! - `try_mark_value_or_raw` — the conservative native-stack scan, which
//!   additionally resolves *interior* pointers via `enclosing_object`.
//! - `CopyingPointerSet::decode_bits` — the copying minor collector, which
//!   additionally requires 8-byte alignment and a live classification, and
//!   whose mark and rewrite are the same operation (`visit_value_bits`), so
//!   it cannot disagree with itself.

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

/// Mark the object referenced by a mutable-root slot word — shadow-stack
/// slots (`MutableRootSlotKind::ShadowStack`) and registered module-global
/// roots (`MutableRootSlotKind::GlobalRoot`) alike.
///
/// Both slot kinds are rewritten by `rewrite_mutable_root_slots` through
/// `try_rewrite_value`, so both must be marked through the same decoder
/// (#6910). Marking is address-identical for the two forms once decoded —
/// `try_mark_raw_root_addr` performs the same validation and mark that
/// `try_mark_value` does after unwrapping a NaN box — so a single call
/// covers them.
#[inline]
pub(super) fn mark_mutable_root_bits(bits: u64, valid_ptrs: &ValidPointerSet) {
    let Some(word) = decode_root_word(bits) else {
        return;
    };
    try_mark_raw_root_addr(word.addr(), valid_ptrs);
}
