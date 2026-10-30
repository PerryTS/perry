//! Where a root scan's native stack walk starts.
//!
//! A runtime entry that JS calls directly and that can collect (the back-edge
//! poll `js_gc_loop_safepoint`, `gc()`) is entered through a shim that hands
//! the body its caller's frame: the JS frame's return address, its stack
//! pointer at the call and its frame pointer. The body passes that frame into
//! the collection it requests, as a field of the request
//! ([`super::super::policy::GcTriggerSnapshot::origin`]), and the collection
//! publishes it for exactly its own synchronous duration. Its scans then start
//! at the JS frame and never step the collector's own Rust frames, which hold
//! no roots: statepoint records are the only root mechanism, and those frames
//! have none.
//!
//! Scoping is what makes this sound, so both halves are guards that restore
//! what they replaced:
//!
//! * [`EntryOrigin`] lives for the entry body and offers the frame to every
//!   collection request the body makes. That is sound because an entry that
//!   offers its frame runs no JS before it returns (the poll, `gc()` and the
//!   allocation slow paths run collector and allocator code only): every
//!   frame between such a request and the offered frame is the runtime's own,
//!   and holds no roots. An entry that can call back into JS must not offer.
//! * [`ActiveOrigin`] lives for one collection (every synchronous collection
//!   entry and every budgeted step installs one). A nested collection
//!   installs its own, so a scan always reads the origin of the collection it
//!   belongs to, and a collection that was not handed one walks from the scan
//!   itself.
//!
//! The frame is never stored past either guard, so a walk can only start at
//! a frame that is live and beneath the scan: the entry's caller, while the
//! entry runs.

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

crate::perry_thread_local! {
    static OFFERED: Cell<Option<WalkOrigin>> = const { Cell::new(None) };
    static ACTIVE: Cell<Option<WalkOrigin>> = const { Cell::new(None) };
}

/// Offers the entry's caller frame to the collection requests of its body.
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
pub(crate) struct EntryOrigin(Option<WalkOrigin>);

#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
impl EntryOrigin {
    #[inline]
    pub(crate) fn enter(origin: WalkOrigin) -> Self {
        Self(OFFERED.with(|cell| cell.replace(Some(origin))))
    }
}

impl Drop for EntryOrigin {
    #[inline]
    fn drop(&mut self) {
        OFFERED.with(|cell| cell.set(self.0));
    }
}

/// The frame the running entry offers, if any: a collection request copies
/// it into itself.
#[inline]
pub(crate) fn offered() -> Option<WalkOrigin> {
    OFFERED.with(Cell::get)
}

/// Publishes one collection's origin (or none) to its scans.
pub(crate) struct ActiveOrigin(Option<WalkOrigin>);

impl ActiveOrigin {
    #[inline]
    pub(crate) fn enter(origin: Option<WalkOrigin>) -> Self {
        Self(ACTIVE.with(|cell| cell.replace(origin)))
    }
}

impl Drop for ActiveOrigin {
    #[inline]
    fn drop(&mut self) {
        ACTIVE.with(|cell| cell.set(self.0));
    }
}

/// The running collection's origin, if its request carried one.
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
#[inline]
pub(crate) fn active() -> Option<WalkOrigin> {
    ACTIVE.with(Cell::get)
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
            assert_eq!(offered(), Some(A), "every request of the entry carries it");
        }
        assert_eq!(offered(), None, "the offer ends with its entry");
    }

    #[test]
    fn a_nested_collection_reads_its_own_origin() {
        let _outer = ActiveOrigin::enter(Some(A));
        assert_eq!(active(), Some(A));
        {
            let _inner = ActiveOrigin::enter(None);
            assert_eq!(active(), None);
        }
        assert_eq!(active(), Some(A));
    }
}
