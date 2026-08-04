//! What `_Unwind_GetCFA` actually returns, pinned by measurement (#7392).
//!
//! The Itanium fallback walker resolves every SP-relative root against this one
//! value, and the number it hands back is not the number its name suggests.
//! DWARF defines a frame's CFA as the *caller's* stack pointer before the call,
//! which is why `unwind::walk_frame` used to compute `CFA - stack_size` to get
//! back down to the frame's own stack pointer. Measured, that subtraction is
//! wrong: inside an `_Unwind_Backtrace` callback the CFA already IS the stack
//! pointer of the frame whose return address `_Unwind_GetIP` reports, so the
//! subtraction moved every root one whole frame too low — reading, and under
//! evacuation *writing*, words that belong to a dead callee frame.
//!
//! Nothing downstream can catch that: no code knows what a root slot is
//! supposed to contain, so wrong words look exactly like right ones. The bug
//! survived because on aarch64 this walker is only the fallback — the x29 chain
//! normally answers first — and it surfaced on aarch64-Linux only once a legal
//! 8-mod-16 frame record made the chain walk bail into it (#7392).
//!
//! So the contract is asserted here rather than trusted: each frame records its
//! real stack pointer on the way in, the walk records the CFA it is given, and
//! the test requires the second list to contain the first, in order. It runs on
//! every host `cargo test` covers — where the two implementations differ
//! (libgcc on Linux, Apple's libunwind on Darwin) and where the return-address
//! convention differs (x86-64 pushes it, aarch64 does not).

use std::ffi::c_void;
use std::hint::black_box;

#[repr(C)]
struct UnwindContext {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn _Unwind_Backtrace(
        trace: unsafe extern "C" fn(*mut UnwindContext, *mut c_void) -> i32,
        argument: *mut c_void,
    ) -> i32;
    fn _Unwind_GetCFA(context: *mut UnwindContext) -> usize;
}

/// The stack pointer of the frame that expands this, which is why it is a macro
/// and not a function: as a function it is only inlined once the optimiser is
/// on, and at `opt-level=0` — the profile `cargo test` uses — it reported its
/// OWN frame instead of its caller's, so every recorded value was wrong by one
/// frame and the test failed for a reason that had nothing to do with the
/// walker. Expanding at the call site takes the question away entirely.
macro_rules! stack_pointer {
    () => {{
        let sp: usize;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            std::arch::asm!("mov {sp}, sp", sp = out(reg) sp, options(nomem, nostack));
        }
        #[cfg(target_arch = "x86_64")]
        unsafe {
            std::arch::asm!("mov {sp}, rsp", sp = out(reg) sp, options(nomem, nostack));
        }
        sp
    }};
}

/// `_URC_NO_REASON`, the only code that continues the walk.
const URC_NO_REASON: i32 = 0;
/// `_URC_END_OF_STACK`. Any non-zero code stops `_Unwind_Backtrace`; this is
/// the one that says "stop, normally".
const URC_END_OF_STACK: i32 = 5;

unsafe extern "C" fn collect(context: *mut UnwindContext, argument: *mut c_void) -> i32 {
    let out = unsafe { &mut *argument.cast::<Vec<usize>>() };
    out.push(unsafe { _Unwind_GetCFA(context) });
    // Bounded: a runaway walk must fail the assertion, not hang the suite.
    if out.len() > 24 {
        URC_END_OF_STACK
    } else {
        URC_NO_REASON
    }
}

fn walk_cfas() -> Vec<usize> {
    let mut cfas: Vec<usize> = Vec::new();
    unsafe {
        _Unwind_Backtrace(collect, (&mut cfas as *mut Vec<usize>).cast::<c_void>());
    }
    cfas
}

// Three frames deep, each with locals the optimiser cannot fold away, so the
// stack pointers are distinct and the frames cannot be merged into one.
#[inline(never)]
fn innermost(sps: &mut Vec<usize>) -> Vec<usize> {
    let pad = black_box([0u64; 4]);
    sps.push(stack_pointer!());
    let cfas = walk_cfas();
    black_box(pad);
    cfas
}

#[inline(never)]
fn middle(sps: &mut Vec<usize>) -> Vec<usize> {
    let pad = black_box([0u64; 16]);
    sps.push(stack_pointer!());
    let cfas = innermost(sps);
    black_box(pad);
    cfas
}

#[inline(never)]
fn outermost(sps: &mut Vec<usize>) -> Vec<usize> {
    let pad = black_box([0u64; 32]);
    sps.push(stack_pointer!());
    let cfas = middle(sps);
    black_box(pad);
    cfas
}

/// The CFA a backtrace callback reports is the frame's own stack pointer.
///
/// Recorded innermost-first, which is the order `_Unwind_Backtrace` visits, and
/// matched as a SUBSEQUENCE: the walk legitimately reports frames these helpers
/// know nothing about (`walk_cfas` itself, the test harness, libc), and adding
/// one of those must not fail the test. What must never happen is a reported
/// CFA that is one frame off from the real stack pointer — the shape of #7392.
#[test]
fn unwind_cfa_is_the_frames_stack_pointer() {
    let mut sps: Vec<usize> = Vec::new();
    let cfas = outermost(&mut sps);
    assert!(
        cfas.len() >= sps.len(),
        "the walk visited {} frame(s) for {} recorded frames",
        cfas.len(),
        sps.len()
    );

    // `sps` is outermost-first (each frame pushes on the way in); the walk is
    // innermost-first.
    let expected: Vec<usize> = sps.iter().rev().copied().collect();
    let mut next = 0usize;
    for cfa in &cfas {
        if next < expected.len() && *cfa == expected[next] {
            next += 1;
        }
    }
    assert_eq!(
        next,
        expected.len(),
        "every recorded stack pointer must appear as a reported CFA, in walk \
         order.\n  recorded stack pointers (innermost first): {expected:#x?}\n  \
         reported CFAs: {cfas:#x?}\nA CFA that is one frame size away from a \
         recorded stack pointer is #7392: `unwind::walk_frame` would then place \
         every SP-relative root in the wrong frame."
    );
}

/// A frame record is two 64-bit words: eight-byte aligned, not sixteen.
///
/// LLVM's AArch64 ELF frame lowering puts the `x29,x30` pair below the other
/// callee-saved GPRs, so an odd number of those lands the pair 8 mod 16 —
/// measured in a real runtime frame in #7392, where the fast walker rejected it
/// as corrupt and bailed into the (then broken) unwinder fallback.
#[test]
fn a_frame_record_needs_only_eight_byte_alignment() {
    let mask = super::FRAME_RECORD_ALIGN_MASK;
    assert_eq!(0x1000usize & mask, 0, "16-aligned records stay valid");
    assert_eq!(
        0x1008usize & mask,
        0,
        "an 8-mod-16 frame record is legal AAPCS64 and must be walked, not \
         rejected — rejecting it is what abandoned the walk in #7392"
    );
    assert_ne!(
        0x1004usize & mask,
        0,
        "a 4-aligned address is still garbage"
    );
}
