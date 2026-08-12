//! The two aarch64 walkers, run against one frame whose layout we chose.
//!
//! # Why this exists
//!
//! Before this file, **nothing `cargo test` runs ever called `fp_chain::visit`
//! or `unwind::visit`.** Their only coverage was `PERRY_STACKMAP_WALKER=verify`
//! inside one arm of `gc-native-roots.yml` — a workflow that had never had a
//! successful run on any branch (#7970), and whose aarch64-ELF arm is red on
//! #7984 precisely because the two walkers resolve one root 96 bytes apart.
//! Every unit test around them tested the *decoder* and the *matcher*; the part
//! that turns a `(register, offset)` pair into a stack address had none.
//!
//! # What it asserts, and why set equality is not enough
//!
//! `verify` compares the two walkers' slot sets. Two empty sets are equal, and
//! so are two identically-wrong sets — that is a presence check, not a proof.
//! The discriminating quantity here is the **contents** of the resolved slot:
//! the probe frame writes a sentinel into the exact word its synthetic
//! stack-map record names, and each walker must independently land on a word
//! holding that sentinel. A walker that visits nothing fails; a walker that
//! visits the wrong word fails even if the other walker agrees with it.
//!
//! `a_wrong_frame_offset_is_caught` is the sabotage arm: it feeds a record
//! whose offset is deliberately 8 bytes off and requires the sentinel check to
//! reject it. Without that, a green run here would only mean the assertions
//! never ran.
//!
//! # The frame layouts
//!
//! Both are copied from real generated code for the same TypeScript function,
//! because they differ per object format and the difference is load-bearing:
//! LLVM's AArch64 **ELF** frame lowering puts the `x29,x30` pair *below* the
//! other callee-saved registers, so the frame record sits in the MIDDLE of the
//! frame and `x29 - body_sp` is small; Mach-O puts it at the TOP. Measured on
//! `benchmarks/gc_ratchet/probes/01_nursery_churn.ts`, the anon-shape
//! constructor: ELF `sub sp,sp,#96 / stp x29,x30,[sp,#40] / add x29,sp,#40`,
//! Mach-O `sub sp,sp,#112 / stp x29,x30,[sp,#96] / add x29,sp,#96`. Both are
//! exercised here on whichever host runs the suite, since the walkers care
//! about the layout and not about the object format that motivated it.

use super::{
    fp_chain, index_records, unwind, ResolvedRoot, StackMapIndex, StackMapLocation, StackMapRecord,
    DWARF_REG_SP_AARCH64,
};

/// Written into the slot the synthetic record names, and required to be there
/// when a walker resolves it. Not a round number: a walker that lands on
/// unrelated stack words must not be able to pass by luck.
const SENTINEL: u64 = 0xCAFE_F00D;

#[cfg(target_vendor = "apple")]
macro_rules! asm_symbol {
    ($name:literal) => {
        concat!("_", $name)
    };
}
#[cfg(not(target_vendor = "apple"))]
macro_rules! asm_symbol {
    ($name:literal) => {
        $name
    };
}

// Each probe: establish the frame, write SENTINEL at [body_sp + 8] — the word
// an `Indirect [R#31 + 8]` record names — then call the Rust callback with
// (body_sp, return_address). `adr x1, 2f` hands the callback the exact PC the
// walkers will match a record on, so the test never has to guess it.
core::arch::global_asm!(
    ".p2align 4",
    concat!(".globl ", asm_symbol!("perry_walker_probe_elf")),
    concat!(asm_symbol!("perry_walker_probe_elf"), ":"),
    ".cfi_startproc",
    "sub sp, sp, #96",
    ".cfi_def_cfa_offset 96",
    "stp x29, x30, [sp, #40]",
    "add x29, sp, #40",
    ".cfi_def_cfa w29, 56",
    ".cfi_offset w30, -48",
    ".cfi_offset w29, -56",
    "mov x2, x0",
    "mov x3, #0xF00D",
    "movk x3, #0xCAFE, lsl #16",
    "str x3, [sp, #8]",
    "mov x0, sp",
    "adr x1, 2f",
    "blr x2",
    "2:",
    "ldp x29, x30, [sp, #40]",
    "add sp, sp, #96",
    "ret",
    ".cfi_endproc",
    ".p2align 4",
    concat!(".globl ", asm_symbol!("perry_walker_probe_darwin")),
    concat!(asm_symbol!("perry_walker_probe_darwin"), ":"),
    ".cfi_startproc",
    "sub sp, sp, #112",
    ".cfi_def_cfa_offset 112",
    "stp x29, x30, [sp, #96]",
    "add x29, sp, #96",
    ".cfi_def_cfa w29, 16",
    ".cfi_offset w30, -8",
    ".cfi_offset w29, -16",
    "mov x2, x0",
    "mov x3, #0xF00D",
    "movk x3, #0xCAFE, lsl #16",
    "str x3, [sp, #8]",
    "mov x0, sp",
    "adr x1, 3f",
    "blr x2",
    "3:",
    "ldp x29, x30, [sp, #96]",
    "add sp, sp, #112",
    "ret",
    ".cfi_endproc",
);

type Probe = extern "C" fn(body_sp: usize, return_address: usize);

unsafe extern "C" {
    fn perry_walker_probe_elf(callback: Probe);
    fn perry_walker_probe_darwin(callback: Probe);
}

/// What the probe frame told us about itself, plus the frame offset the
/// synthetic record should carry.
#[derive(Clone, Copy)]
struct Frame {
    function_address: usize,
    stack_size: u64,
    /// `x29 - body_sp` for this layout, for the message when a walk is wrong.
    fp_to_sp: usize,
}

const ELF_FRAME: Frame = Frame {
    function_address: 0,
    stack_size: 96,
    fp_to_sp: 40,
};
const DARWIN_FRAME: Frame = Frame {
    function_address: 0,
    stack_size: 112,
    fp_to_sp: 96,
};

fn index_for(frame: Frame, return_address: usize, offset: i32) -> StackMapIndex {
    index_records(
        vec![StackMapRecord {
            pc: return_address,
            function_address: frame.function_address,
            stack_size: frame.stack_size,
            roots_start: 0,
            roots_len: 1,
        }],
        vec![StackMapLocation {
            dwarf_reg: DWARF_REG_SP_AARCH64,
            offset,
        }],
    )
}

/// One resolved slot, sampled WHILE THE PROBE FRAME IS STILL LIVE.
///
/// Reading the word after the probe returns reads a dead frame that the test
/// harness has already reused — measured while writing this file, as a
/// sentinel that had become `4`. The walk and the sample therefore both happen
/// inside the callback, and only the values travel back out.
#[derive(Clone, Copy, Debug)]
struct Sample {
    address: usize,
    word: u64,
}

fn sample(address: usize) -> Sample {
    Sample {
        address,
        // The walkers just handed this address to a collector that would WRITE
        // through it, so reading it is strictly weaker than what production
        // does with the same value.
        word: unsafe { std::ptr::read(address as *const u64) },
    }
}

fn walk(index: &StackMapIndex) -> (Option<Vec<Sample>>, Vec<Sample>) {
    let mut fast: Vec<Sample> = Vec::new();
    let fast_stats = fp_chain::visit(index, &mut |root: ResolvedRoot| {
        fast.push(sample(root.address))
    });
    let mut slow: Vec<Sample> = Vec::new();
    unwind::visit(index, &mut |root: ResolvedRoot| {
        slow.push(sample(root.address))
    });
    (fast_stats.map(|_| fast), slow)
}

/// Every walker must land on a word holding `SENTINEL`, and must land on at
/// least one word at all.
fn check(kind: &str, walker: &str, samples: &[Sample], expected: usize, frame: Frame) {
    assert!(
        !samples.is_empty(),
        "{kind}: the {walker} walker resolved NO root. Set equality between \
         two walkers is satisfied by both finding nothing, which is why this \
         asserts the walk reached the probe frame rather than that the two \
         agreed."
    );
    for Sample { address, word } in samples {
        assert_eq!(
            *address,
            expected,
            "{kind}: the {walker} walker placed the root at {address:#x}, not \
             {expected:#x} ({} bytes out). The probe frame is \
             `x29 - body_sp = {}`; a constant miss is a frame-base error, \
             which is #7984's shape.",
            *address as i64 - expected as i64,
            frame.fp_to_sp,
        );
        // The discriminating quantity. Two walkers agreeing on the wrong word
        // is exactly the failure `verify`'s set comparison cannot see.
        assert_eq!(
            *word, SENTINEL,
            "{kind}: the {walker} walker resolved {address:#x}, which does not \
             hold the sentinel the probe frame wrote into the word its record \
             names — the address is a stack word, but not the root's."
        );
    }
}

// The probe frame reports itself through this cell: an `extern "C"` callback
// has nowhere else to put a result, and a thread-local keeps it off `static
// mut` (whose references the 2024 edition rejects). Each test drives one probe
// to completion before reading, so there is no interleaving to reason about.
thread_local! {
    static PROBE: std::cell::RefCell<ProbeState> = const {
        std::cell::RefCell::new(ProbeState {
            frame: ELF_FRAME,
            offset: 8,
            body_sp: 0,
            fast: None,
            slow: None,
        })
    };
}

struct ProbeState {
    frame: Frame,
    offset: i32,
    body_sp: usize,
    fast: Option<Vec<Sample>>,
    slow: Option<Vec<Sample>>,
}

extern "C" fn run_probe(body_sp: usize, return_address: usize) {
    let (frame, offset) = PROBE.with(|cell| {
        let state = cell.borrow();
        (state.frame, state.offset)
    });
    let index = index_for(frame, return_address, offset);
    let (fast, slow) = walk(&index);
    let fast = fast.expect(
        "the fp-chain walk returned None (an anomaly bail-out). A walker that \
         declines to run cannot be cross-checked against the other, which is \
         the other half of what `verify` reports.",
    );
    PROBE.with(|cell| {
        let mut state = cell.borrow_mut();
        state.body_sp = body_sp;
        state.fast = Some(fast);
        state.slow = Some(slow);
    });
}

fn drive(
    probe: unsafe extern "C" fn(Probe),
    frame: Frame,
    offset: i32,
) -> (usize, Vec<Sample>, Vec<Sample>) {
    PROBE.with(|cell| {
        let mut state = cell.borrow_mut();
        state.frame = frame;
        state.offset = offset;
        state.fast = None;
        state.slow = None;
    });
    unsafe { probe(run_probe) };
    PROBE.with(|cell| {
        let mut state = cell.borrow_mut();
        let fast = state.fast.take().expect("the probe callback never ran");
        let slow = state.slow.take().expect("the probe callback never ran");
        (state.body_sp, fast, slow)
    })
}

/// `function_address` is the symbol's runtime address, which only Rust knows.
fn elf_frame() -> Frame {
    Frame {
        function_address: perry_walker_probe_elf as *const () as usize,
        ..ELF_FRAME
    }
}

fn darwin_frame() -> Frame {
    Frame {
        function_address: perry_walker_probe_darwin as *const () as usize,
        ..DARWIN_FRAME
    }
}

#[test]
fn both_walkers_resolve_an_sp_root_in_an_elf_shaped_frame() {
    let frame = elf_frame();
    let (body_sp, fast, slow) = drive(perry_walker_probe_elf, frame, 8);
    let expected = body_sp + 8;
    check("ELF-shaped frame", "fp-chain", &fast, expected, frame);
    check("ELF-shaped frame", "unwinder", &slow, expected, frame);
}

#[test]
fn both_walkers_resolve_an_sp_root_in_a_darwin_shaped_frame() {
    let frame = darwin_frame();
    let (body_sp, fast, slow) = drive(perry_walker_probe_darwin, frame, 8);
    let expected = body_sp + 8;
    check("Mach-O-shaped frame", "fp-chain", &fast, expected, frame);
    check("Mach-O-shaped frame", "unwinder", &slow, expected, frame);
}

/// The sabotage arm: prove the sentinel check can fail.
///
/// A gate whose assertions have never been violated is a gate nobody has shown
/// can fail. Feed a record whose frame offset is 8 bytes past the real slot and
/// require BOTH walkers to land somewhere that does not hold the sentinel — if
/// this test ever passes by finding the sentinel anyway, the checks above prove
/// nothing.
#[test]
fn a_wrong_frame_offset_is_caught() {
    let (body_sp, fast, slow) = drive(perry_walker_probe_elf, elf_frame(), 16);
    assert!(
        !fast.is_empty() && !slow.is_empty(),
        "both walkers must still run"
    );
    for Sample { address, word } in fast.iter().chain(slow.iter()) {
        assert_eq!(
            *address,
            body_sp + 16,
            "the sabotaged record must resolve to the sabotaged address"
        );
        assert_ne!(
            *word, SENTINEL,
            "the sentinel check cannot distinguish the right slot from a wrong \
             one, so the agreement tests above prove nothing"
        );
    }
}
