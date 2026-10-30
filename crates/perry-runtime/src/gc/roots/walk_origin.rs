//! Where a root scan's native stack walk starts.
//!
//! A runtime entry that JS calls directly and that can collect (the back-edge
//! poll `js_gc_loop_safepoint`, `gc()`, the inline allocation slow paths) is
//! entered through a shim that hands the body its caller's frame: the JS
//! frame's return address, its stack pointer at the call and its frame
//! pointer. While the body runs it offers that frame ([`EntryOrigin`]), and a
//! scan that runs meanwhile starts its walk there instead of stepping the
//! collector's own Rust frames, which hold no roots: statepoint records are
//! the only root mechanism, and those frames have none.
//!
//! What makes this sound is what an offering entry does not do: it runs no JS
//! before it returns (the poll, `gc()` and allocation run collector and
//! allocator code only). So every frame between any scan made during the
//! entry and the offered frame is the runtime's own, whichever collection,
//! nested collection or budgeted step the scan belongs to. An entry that can
//! call back into JS must not offer.
//!
//! The offer ends with the entry: [`EntryOrigin`] clears it on return (an
//! entry nested in another, a Rust caller of an offering entry, so ends the
//! outer offer too, and the outer body's later scans walk from the scan:
//! never wrong, only slower). A walk can therefore only start at a frame that
//! is live and beneath the scan.

use std::cell::Cell;

/// The JS caller of a collecting entry: `pc` is the return address into it,
/// `sp` its stack pointer at the call (the entry's CFA), `fp` its frame
/// pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WalkOrigin {
    pub(crate) pc: usize,
    pub(crate) sp: usize,
    pub(crate) fp: usize,
}

// `#[thread_local]`, not `thread_local!`: const, drop-free and all-zero (so
// in `.tbss`), it needs no accessor and no claimed slot. One
// thread-pointer-relative load or store.
#[thread_local]
static OFFERED: Cell<Option<WalkOrigin>> = Cell::new(None);

/// Offers the entry's caller frame to the collection requests of its body.
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
pub(crate) struct EntryOrigin(());

#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
impl EntryOrigin {
    #[inline]
    pub(crate) fn enter(origin: WalkOrigin) -> Self {
        OFFERED.set(Some(origin));
        Self(())
    }
}

/// Clears rather than restores: an entry nested in another (a Rust caller of
/// an offering entry) ends the outer offer too, and the outer body's later
/// requests then walk from the scan. Never wrong, only slower, and it spares
/// every entry a load.
impl Drop for EntryOrigin {
    #[inline]
    fn drop(&mut self) {
        OFFERED.set(None);
    }
}

/// The frame the running offering entry passed, if any: where a scan's walk
/// starts.
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
#[inline]
pub(crate) fn offered() -> Option<WalkOrigin> {
    OFFERED.get()
}

/// The body of an x86-64 Linux naked entry that offers its caller's frame: it
/// leaves the entry's own `N` integer arguments in place, passes the return
/// address, the stack pointer at the call and the frame pointer as arguments
/// `N`, `N + 1` and `N + 2`, and tail-jumps to the `_from` body, so the entry
/// never owns a frame. Three moves on paths that are already out of line.
#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
macro_rules! offering_entry_asm {
    (0, $from:path) => {
        core::arch::naked_asm!(
            "mov rdi, qword ptr [rsp]",
            "lea rsi, [rsp + 8]",
            "mov rdx, rbp",
            "jmp {from}",
            from = sym $from,
        )
    };
    (1, $from:path) => {
        core::arch::naked_asm!(
            "mov rsi, qword ptr [rsp]",
            "lea rdx, [rsp + 8]",
            "mov rcx, rbp",
            "jmp {from}",
            from = sym $from,
        )
    };
    (3, $from:path) => {
        core::arch::naked_asm!(
            "mov rcx, qword ptr [rsp]",
            "lea r8, [rsp + 8]",
            "mov r9, rbp",
            "jmp {from}",
            from = sym $from,
        )
    };
}
#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
pub(crate) use offering_entry_asm;

#[cfg(test)]
mod tests {
    use super::*;

    const A: WalkOrigin = WalkOrigin {
        pc: 1,
        sp: 2,
        fp: 3,
    };

    #[test]
    fn an_offer_never_outlives_its_entry() {
        {
            let _entry = EntryOrigin::enter(A);
            assert_eq!(offered(), Some(A));
            {
                let _nested = EntryOrigin::enter(WalkOrigin { pc: 4, ..A });
                assert_eq!(offered().map(|o| o.pc), Some(4));
            }
            assert_eq!(offered(), None, "a nested entry ends the outer offer");
        }
        assert_eq!(offered(), None, "the offer ends with its entry");
    }
}
