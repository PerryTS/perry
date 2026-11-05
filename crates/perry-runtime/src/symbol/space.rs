//! The process immortal symbol space (DECISIONS 243(c)).
//!
//! Every persistent symbol (well-known, `Symbol.for`, Intl, runtime-created)
//! is one fixed-size cell of ONE contiguous range shared by every worker:
//! `GcHeader + SymbolHeader + native description text`. Cells are bump
//! allocated with one atomic add and never freed (registered symbols live as
//! long as the process, by the spec).
//!
//! The range is what lets a RAW word be admitted without reading memory:
//! `base <= addr < bump && (addr - base) % CELL == GC_HEADER_SIZE` names a
//! cell start of this space, and only then may its header and payload be
//! read. A raw word may be a number (`bare_receiver`), so nothing outside the
//! range is ever dereferenced here.
//!
//! The range comes from the runtime's zero-page primitive
//! (`zeroed_cache::map_zero_pages`: anonymous `mmap` on unix, `VirtualAlloc`
//! on windows, a zeroed allocation on wasm32), mapped by the first persistent
//! symbol. On unix a page is backed only when a cell on it is first written,
//! so the space grows in RSS with the symbols the program creates.
//!
//! Bound: [`CELLS`] cells. Each one is a symbol the program made immortal; a
//! `Symbol.for` cell also keeps its registry key alive, so at the bound the
//! program already holds that many keys forever. Reaching it is a clear
//! abort, never a second space and never a scan.

use std::sync::atomic::{AtomicUsize, Ordering};

/// One cell: the GC prefix, the symbol payload and the native text tail.
pub(super) const CELL: usize = std::mem::size_of::<super::PersistentSymbol>();
/// The address range of the space.
pub(super) const SPACE_BYTES: usize = 16 << 20;
/// How many persistent symbols one process can create.
pub(super) const CELLS: usize = SPACE_BYTES / CELL;

const _: () = assert!(CELL % std::mem::align_of::<super::PersistentSymbol>() == 0);
const _: () = assert!(4096 % std::mem::align_of::<super::PersistentSymbol>() == 0);

/// The range start; 0 until the first persistent symbol maps it.
static BASE: AtomicUsize = AtomicUsize::new(0);
/// Bytes handed out from `BASE`. A reader that meets a handed-out cell before
/// it is written reads zero pages: no magic, so no symbol.
static BUMP: AtomicUsize = AtomicUsize::new(0);

#[cold]
#[inline(never)]
fn map_space() -> usize {
    static MAP: std::sync::Once = std::sync::Once::new();
    MAP.call_once(|| {
        // SAFETY: asks for a fresh zeroed, page-aligned range.
        let base = unsafe { crate::zeroed_cache::map_zero_pages(SPACE_BYTES) } as usize;
        if base == 0 {
            std::alloc::handle_alloc_error(
                std::alloc::Layout::from_size_align(SPACE_BYTES, 4096).unwrap(),
            );
        }
        BASE.store(base, Ordering::Release);
    });
    BASE.load(Ordering::Acquire)
}

/// A fresh zeroed cell of the space, for every worker alike.
pub(super) fn alloc_cell() -> *mut super::PersistentSymbol {
    let mut base = BASE.load(Ordering::Acquire);
    if base == 0 {
        base = map_space();
    }
    let offset = BUMP.fetch_add(CELL, Ordering::AcqRel);
    if offset + CELL > SPACE_BYTES {
        exhausted();
    }
    (base + offset) as *mut super::PersistentSymbol
}

#[cold]
#[inline(never)]
fn exhausted() -> ! {
    eprintln!(
        "perry: the process symbol space is full: {CELLS} immortal symbols \
         (Symbol.for keys, well-known, Intl and runtime symbols)"
    );
    std::process::abort()
}

/// Is the raw word `addr` the user address (`header + GC_HEADER_SIZE`) of a
/// handed-out cell of the space? Reads no memory: the word may be a number.
#[inline(always)]
pub(super) fn holds(addr: usize) -> bool {
    let base = BASE.load(Ordering::Acquire);
    let offset = addr.wrapping_sub(base);
    base != 0
        && offset < BUMP.load(Ordering::Acquire).min(SPACE_BYTES)
        && offset % CELL == crate::gc::GC_HEADER_SIZE
}

#[cfg(test)]
pub(super) fn base() -> usize {
    BASE.load(Ordering::Acquire)
}
