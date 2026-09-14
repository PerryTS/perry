//! #10182: a synchronous full zeroes the dead stack region below its frame
//! after the census walk and before the root scan (`scrub_dead_stack_below`).
//!
//! A conservative stack scan reads every word from its own stack pointer up,
//! including slots of live frames that were never written since a deeper call
//! returned, so a heap address a returned census frame left there reads as a
//! root. The case plants a sentinel in dead stack exactly where a returned
//! call leaves its frame, and reads that region back from the same depth:
//! without the scrub the sentinel is still there, with it the region is zero.

use super::super::cycle::scrub_dead_stack_below;

const SENTINEL: u64 = 0x0000_7fee_dead_beef;
const WORDS: usize = 1024;

#[inline(never)]
fn plant_sentinels() {
    let mut words = [SENTINEL; WORDS];
    std::hint::black_box(&mut words);
}

#[inline(always)]
fn stack_pointer() -> usize {
    let sp: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, sp", out(reg) sp);
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("mov {}, rsp", out(reg) sp);
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        sp = 0;
    }
    sp
}

/// Sentinels left in the `WORDS` words below this frame's stack pointer.
#[inline(never)]
fn sentinels_below_after(scrub: bool) -> usize {
    plant_sentinels();
    if scrub {
        scrub_dead_stack_below();
    }
    let sp = stack_pointer();
    if sp == 0 {
        return usize::from(!scrub);
    }
    (1..=WORDS)
        .filter(|&i| {
            // SAFETY: the region below the stack pointer belongs to this
            // thread's stack mapping; it is dead, not unmapped.
            unsafe { std::ptr::read_volatile((sp - i * 8) as *const u64) == SENTINEL }
        })
        .count()
}

#[test]
fn the_scrub_erases_what_a_returned_frame_left_below_the_stack_pointer() {
    let left = sentinels_below_after(false);
    assert!(
        left > 0,
        "premise: a returned frame leaves its words in dead stack"
    );
    assert_eq!(
        sentinels_below_after(true),
        0,
        "the scrub must zero the dead region the next call chain will occupy"
    );
}
