//! `PERRY_STACKMAP_WALKER=verify`: run both walkers and require them to agree.
//!
//! This is the only check that can catch a fast walk that silently skips
//! frames or resolves a root against the wrong frame base. Forced-evacuation
//! verification cannot: it enumerates roots through the same walker, so it
//! never sees a slot the walker never reached, and it has no idea what a root
//! slot is *supposed* to contain — a wrong stack word looks exactly like a
//! right one.
//!
//! # Why the report is this detailed
//!
//! The first end-to-end `verify` run on aarch64 ELF caught the fp-chain walker
//! and the unwinder resolving one root 96 bytes apart (#7984). All the panic
//! could say was
//!
//! ```text
//! fast walk visited 1 unique slots, unwinder visited 1
//!   left:  [281474742909688]
//!   right: [281474742909592]
//! ```
//!
//! — two integers, from which the frame, the base register, the function whose
//! prologue was decoded, and therefore *which walker was wrong* are all
//! unrecoverable. Every candidate explanation (a missed trailing `sub sp`, a
//! frame the chain skipped, a CFA that is one frame out) predicts exactly that
//! output, so the gate could prove a bug existed and nothing about its shape.
//!
//! A gate that cannot name what it found sends whoever picks it up back to
//! square one, so this one prints the provenance of every root both walkers
//! resolved: the frame return address it was matched on, the function the
//! record belongs to, the base register and frame offset from the map, the
//! base each walker resolved that register to, and — on aarch64 — the
//! prologue words `fp_to_sp_offset` decoded to derive an SP base. That is
//! enough to say which walker is wrong without a second run.

use super::{fp_chain, unwind, MutableRootSlot, NativeStackWalkStats, ResolvedRoot, StackMapIndex};
use std::fmt::Write as _;

/// Run the fast walk non-mutating, then the unwinder for the real visitation,
/// and panic unless they resolved the identical set of slot addresses.
pub(super) fn visit(
    index: &StackMapIndex,
    visit: &mut impl FnMut(MutableRootSlot),
) -> NativeStackWalkStats {
    let mut fast: Vec<ResolvedRoot> = Vec::new();
    let fast_stats = fp_chain::visit(index, &mut |root: ResolvedRoot| fast.push(root));
    let Some(fast_stats) = fast_stats else {
        panic!(
            "PERRY_STACKMAP_WALKER=verify: fast walk unavailable \
             (chain_walkable={}, anomaly or unsupported target)",
            index.chain_walkable
        );
    };
    let mut slow: Vec<ResolvedRoot> = Vec::new();
    let mut stats = unwind::visit(index, &mut |root: ResolvedRoot| {
        slow.push(root);
        visit(MutableRootSlot {
            kind: super::MutableRootSlotKind::NativeStack,
            ptr: root.address as *mut u64,
        });
    });

    if !addresses_agree(&fast, &slow) {
        panic!("{}", report(index, &fast, &slow));
    }

    stats.fp_walks = fast_stats.fp_walks;
    stats
}

/// The comparison the gate actually makes: the SETS of slot addresses, since
/// visiting order and duplicate visits of one slot are both immaterial to the
/// collector (rewriting a slot twice is idempotent).
fn addresses_agree(fast: &[ResolvedRoot], slow: &[ResolvedRoot]) -> bool {
    unique_addresses(fast) == unique_addresses(slow)
}

fn unique_addresses(roots: &[ResolvedRoot]) -> Vec<usize> {
    let mut out: Vec<usize> = roots.iter().map(|root| root.address).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The full disagreement, one line per root, plus the prologue evidence for
/// every function whose frame either walker resolved an SP-relative root in.
fn report(index: &StackMapIndex, fast: &[ResolvedRoot], slow: &[ResolvedRoot]) -> String {
    let fast_addresses = unique_addresses(fast);
    let slow_addresses = unique_addresses(slow);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "PERRY_STACKMAP_WALKER=verify: the fp-chain walker and the platform \
         unwinder resolved different root slots.\n  \
         fp-chain: {} unique slot(s) {:#x?}\n  unwinder: {} unique slot(s) {:#x?}",
        fast_addresses.len(),
        fast_addresses,
        slow_addresses.len(),
        slow_addresses,
    );
    // A constant delta between two same-length sets is the signature of a
    // frame-base disagreement rather than a missed or invented frame, and it
    // is the first thing to know. Say so explicitly instead of leaving it to
    // be spotted by subtracting two decimal integers by hand.
    if fast_addresses.len() == slow_addresses.len() {
        let deltas: Vec<i64> = fast_addresses
            .iter()
            .zip(&slow_addresses)
            .map(|(fast, slow)| *fast as i64 - *slow as i64)
            .collect();
        let _ = writeln!(
            out,
            "  same slot count, so this is a base disagreement, not a missed \
             frame; fp-chain minus unwinder = {deltas:?} byte(s)"
        );
    }
    let _ = writeln!(out, "\n  fp-chain roots:");
    for root in fast {
        describe(&mut out, index, root);
    }
    let _ = writeln!(out, "  unwinder roots:");
    for root in slow {
        describe(&mut out, index, root);
    }
    out
}

fn describe(out: &mut String, index: &StackMapIndex, root: &ResolvedRoot) {
    let register = match root.dwarf_reg {
        29 => "fp/x29",
        31 => "sp",
        _ => "reg",
    };
    let _ = writeln!(
        out,
        "    slot {:#x} = base {:#x} {:+} | ip {:#x} (fn {:#x} + {:#x}) | \
         map: dwarf {} ({}) offset {:+}{}",
        root.address,
        root.base,
        root.offset,
        root.ip,
        root.function_address,
        root.ip.wrapping_sub(root.function_address),
        root.dwarf_reg,
        register,
        root.offset,
        prologue_note(index, root),
    );
}

/// Whether the map itself vouches for `function_address` as the start of a
/// function it has records for.
///
/// The report runs on the failure path, where a plausible cause is a map whose
/// addresses are wrong — so it must not dereference an address on the strength
/// of the very data under suspicion. This is the same set the walker's
/// `match_records` containment check consults, so a dump gated on it reads
/// only what the walk already read.
fn map_vouches_for(index: &StackMapIndex, function_address: usize) -> bool {
    index
        .function_starts
        .binary_search(&function_address)
        .is_ok()
}

/// What the fast walker derives an SP base from, spelled out.
///
/// `fp_to_sp_offset` decodes the owning function's prologue to get
/// `x29 - body_sp`; every SP-relative root in that frame is placed relative to
/// the result. When it is wrong the whole frame is wrong by one constant,
/// which is precisely the shape #7984 presents, so the decoded value and the
/// words it was decoded from belong in the report.
#[cfg(target_arch = "aarch64")]
fn prologue_note(index: &StackMapIndex, root: &ResolvedRoot) -> String {
    if root.dwarf_reg != super::DWARF_REG_SP_AARCH64
        || !map_vouches_for(index, root.function_address)
    {
        return String::new();
    }
    let decoded = super::fp_to_sp_offset(root.function_address);
    let mut words = String::new();
    for word_index in 0..10usize {
        // Reading the prologue is what the walker itself does, from the same
        // address it was already gated on above, so this adds no unsafety the
        // walk did not already have.
        let word =
            unsafe { std::ptr::read((root.function_address + word_index * 4) as *const u32) };
        let _ = write!(words, " {word:08x}");
    }
    format!("\n      fp_to_sp_offset(fn) = {decoded:?}, prologue words:{words}")
}

#[cfg(not(target_arch = "aarch64"))]
fn prologue_note(_index: &StackMapIndex, _root: &ResolvedRoot) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(address: usize, base: usize, offset: i32) -> ResolvedRoot {
        ResolvedRoot {
            address,
            ip: 0x4000,
            function_address: 0x3000,
            dwarf_reg: 31,
            offset,
            base,
        }
    }

    /// An index that vouches for NO function address, so the report never
    /// dereferences the synthetic addresses above.
    fn empty_index() -> StackMapIndex {
        super::super::index_records(Vec::new(), Vec::new())
    }

    #[test]
    fn identical_sets_agree_regardless_of_order_or_repeats() {
        let fast = vec![root(0x100, 0xF8, 8), root(0x200, 0x1F8, 8)];
        let slow = vec![
            root(0x200, 0x1F8, 8),
            root(0x100, 0xF8, 8),
            root(0x100, 0xF8, 8),
        ];
        assert!(
            addresses_agree(&fast, &slow),
            "the collector rewrites a slot idempotently, so order and repeats \
             must not fail the gate"
        );
    }

    #[test]
    fn a_constant_base_delta_is_named_as_one() {
        // #7984's exact shape: one slot each, 96 bytes apart. The report has
        // to say "base disagreement" and print the delta, because that is the
        // fact that separates a wrong frame base from a missed frame — and the
        // old message printed neither.
        let fast = vec![root(0x1060, 0x1058, 8)];
        let slow = vec![root(0x1000, 0xFF8, 8)];
        assert!(!addresses_agree(&fast, &slow));
        let text = report(&empty_index(), &fast, &slow);
        assert!(
            text.contains("base disagreement"),
            "equal slot counts must be reported as a base disagreement: {text}"
        );
        assert!(
            text.contains("[96]"),
            "the report must print the byte delta: {text}"
        );
        assert!(
            text.contains("dwarf 31 (sp)"),
            "the report must name the base register the map asked for: {text}"
        );
    }

    #[test]
    fn a_missed_frame_is_not_reported_as_a_base_disagreement() {
        let fast = vec![root(0x1000, 0xFF8, 8)];
        let slow = vec![root(0x1000, 0xFF8, 8), root(0x2000, 0x1FF8, 8)];
        let text = report(&empty_index(), &fast, &slow);
        assert!(
            !text.contains("base disagreement"),
            "different slot counts mean a frame was missed or invented: {text}"
        );
    }

    /// The report must not dereference an address the map does not vouch for.
    ///
    /// It runs on the failure path, and one live hypothesis for any such
    /// failure is a map whose function addresses are wrong — so reading
    /// instructions from one on the strength of that same map turns a
    /// diagnostic into a SIGSEGV with no output at all. Measured while writing
    /// this file: the first draft did exactly that.
    #[test]
    fn an_unvouched_function_address_is_never_dereferenced() {
        let index = empty_index();
        assert!(!map_vouches_for(&index, 0x3000));
        let text = report(&index, &[root(0x1000, 0xFF8, 8)], &[]);
        assert!(
            !text.contains("prologue words"),
            "no prologue may be dumped for an address the map does not list: {text}"
        );
    }
}
