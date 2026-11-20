//! Give freed linear-memory pages back to a WASI host.
//!
//! A wasm linear memory never shrinks, and nothing the guest can do with its
//! own instructions makes a touched page non-resident: freed arena blocks and
//! freed malloc chunks keep costing host memory forever. A host can drop them
//! (`madvise(MADV_DONTNEED)` on Linux), so a module linked with
//! `PERRY_WASM_DISCARD=1` (the compile-time opt-in, see
//! `perry::commands::compile::link::wasi`) imports
//!
//! ```text
//! (import "perry_host" "discard_pages" (func (param i32 i32) (result i32)))
//! ;; discard_pages(ptr, len) -> 1 when every byte of [ptr, ptr + len) now reads 0
//! ;;                           0 when the host did nothing (contents unchanged)
//! ```
//!
//! and calls it on page-aligned ranges it no longer needs: the interior of a
//! freed allocation of at least [`MIN_FREE_DISCARD`] bytes (every Rust-side
//! free goes through [`DiscardAlloc`], so arena blocks, native buffer
//! backings and large `Vec`s are all covered), arena blocks that sat unused in
//! the block pool for a whole mutator interval, and detached or shrunk
//! ArrayBuffer payloads.
//!
//! This module exists only in a `wasm32-wasip1` runtime built with the
//! `wasm-discard` feature (`scripts/wasi_build_runtime.py`); the WASIp2
//! runtime has neither the allocator wrapper nor the call sites. A link that
//! does not ask for the import still has the code, but its hook answers 0 and
//! the module imports nothing new.
//!
//! Contract the host implements (docs/WASI_P1_OUTPUT.md):
//! - `ptr` and `len` are multiples of [`PAGE`] (4096) and `len > 0`. The host
//!   traps on a range outside the memory. A host whose page is larger than
//!   4096 releases the whole host pages inside the range and zeroes the rest.
//! - On a 1 result the range reads as zero afterwards (anonymous private
//!   memory under `MADV_DONTNEED` does; a page of a copy-on-write image
//!   mapping would read the image back, so such a host must zero or remap it).
//! - A 0 result turns discarding off for the rest of the run; it is also what
//!   the module's fallback returns when the import was not linked, and what
//!   `wasmtime -W unknown-imports-default=y` returns.
//!
//! Large frees do not go to the host straight away. A request's buffers are
//! freed and allocated again within milliseconds, and a discarded page that
//! is reused costs the host a page fault and a zero-fill, so discarding on
//! every free would turn each byte a program moves into several faulted bytes.
//! [`DiscardAlloc`] keeps up to [`KEEP_CAP`] bytes of freed blocks (at most
//! [`KEEP_SLOTS`] of them) allocated and resident, hands them back out to large
//! allocations that fit, and discards a block only when it has sat unused for
//! [`KEEP_IDLE`] (checked when the event loop is about to park, [`idle`]) or
//! the budget is full (the oldest goes first). A loop that never parks (one
//! busy with immediates) keeps its blocks until they are reused or evicted,
//! never more than the budget. An allocation malloc cannot serve releases the
//! whole list and tries again.
//!
//! The guest never relies on a page staying discarded: it rewrites every
//! byte it reuses, and the one caller that wants zeros
//! ([`discard_zeroed`]) asks for them from the result.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

extern "C" {
    /// `src/ffi/perry_wasm_discard.c`: the host import when it was linked,
    /// else 0.
    fn perry_wasm_discard(ptr: *mut u8, len: usize) -> i32;
    /// Whether the module was linked with the import.
    fn perry_wasm_discard_linked() -> i32;
}

/// Granularity of a discard request.
pub(crate) const PAGE: usize = 4096;
/// Smallest free whose interior is discarded. Below it a free cannot hold
/// more than a few whole pages, and the host call costs more than they do.
pub(crate) const MIN_FREE_DISCARD: usize = 64 * 1024;
/// Bytes left alone at both ends of a range discarded AFTER the allocator
/// took it back: dlmalloc writes a free chunk's links just past its header
/// and its size into the last word, all well inside this.
const FREE_MARGIN: usize = 64;

/// Most freed blocks held back for reuse at once.
const KEEP_SLOTS: usize = 256;
/// How long a held-back block may sit unused before it is discarded.
const KEEP_IDLE: Duration = Duration::from_secs(1);
/// Most bytes of freed blocks held back for reuse.
const KEEP_CAP: usize = 8 << 20;

const UNKNOWN: u8 = 0;
const ON: u8 = 1;
const OFF: u8 = 2;
static STATE: AtomicU8 = AtomicU8::new(UNKNOWN);
static DISCARDED: AtomicU64 = AtomicU64::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static REUSED: AtomicUsize = AtomicUsize::new(0);
/// Of those, the ones handed out to a zeroed allocation (and cleared by hand).
static REUSED_ZEROED: AtomicUsize = AtomicUsize::new(0);

/// What asked for a discard, for the `PERRY_GC_LOG` breakdown.
#[derive(Clone, Copy)]
pub(crate) enum Source {
    /// A large free (or the old block of a moving realloc) the keep list
    /// had no room for.
    Free = 0,
    /// The tail a shrinking realloc gave up.
    Shrink = 1,
    /// A held-back block evicted or idle past [`KEEP_IDLE`].
    Kept = 2,
    /// An arena block idle in the block pool for a mutator interval.
    Pool = 3,
    /// A detached or shrunk ArrayBuffer payload.
    Buffer = 4,
}
const SOURCES: [&str; 5] = ["free", "shrink", "kept", "pool", "buffer"];
static SOURCE_CALLS: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
static SOURCE_BYTES: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

/// Whether to ask the host: the import was linked and no earlier call was
/// answered 0.
fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        ON => true,
        OFF => false,
        _ => {
            let linked = unsafe { perry_wasm_discard_linked() } != 0;
            STATE.store(if linked { ON } else { OFF }, Ordering::Relaxed);
            linked
        }
    }
}

#[derive(Clone, Copy)]
struct Slot {
    ptr: usize,
    /// `malloc_usable_size`: what the block can hold, whatever it was
    /// allocated for.
    size: usize,
    /// Order of arrival, so the oldest goes first when the list is full.
    seq: u64,
    freed: Option<Instant>,
}

/// Freed blocks still allocated in wasi-libc's malloc: nobody else can
/// allocate from them, so discarding one later cannot hit live data.
struct Kept {
    slots: [Slot; KEEP_SLOTS],
    len: usize,
    bytes: usize,
    seq: u64,
}

/// wasm32-wasip1 runs one thread, and none of these functions re-enters the
/// global allocator: the list needs no lock.
struct KeptCell(UnsafeCell<Kept>);
unsafe impl Sync for KeptCell {}

static KEPT: KeptCell = KeptCell(UnsafeCell::new(Kept {
    slots: [Slot {
        ptr: 0,
        size: 0,
        seq: 0,
        freed: None,
    }; KEEP_SLOTS],
    len: 0,
    bytes: 0,
    seq: 0,
}));

impl Kept {
    fn remove(&mut self, i: usize) -> Slot {
        let slot = self.slots[i];
        self.len -= 1;
        self.slots[i] = self.slots[self.len];
        self.bytes -= slot.size;
        slot
    }

    fn oldest(&self) -> usize {
        (0..self.len)
            .min_by_key(|&i| self.slots[i].seq)
            .unwrap_or(0)
    }
}

/// Discard a held-back block's pages and give it to malloc.
unsafe fn release(slot: Slot) {
    if enabled() {
        if let Some((start, end)) = interior(slot.ptr, slot.size) {
            request(start, end, Source::Kept);
        }
    }
    libc::free(slot.ptr as *mut libc::c_void);
}

/// Hold a freed block back for reuse instead of freeing it. False when it
/// is not wanted: discarding is off, or the block alone exceeds the budget.
unsafe fn keep(ptr: *mut u8) -> bool {
    if !enabled() {
        return false;
    }
    let size = usable(ptr);
    if size > KEEP_CAP {
        return false;
    }
    let kept = &mut *KEPT.0.get();
    while kept.len == KEEP_SLOTS || kept.bytes + size > KEEP_CAP {
        let i = kept.oldest();
        release(kept.remove(i));
    }
    kept.seq += 1;
    kept.slots[kept.len] = Slot {
        ptr: ptr as usize,
        size,
        seq: kept.seq,
        freed: Some(Instant::now()),
    };
    kept.len += 1;
    kept.bytes += size;
    true
}

/// A held-back block for `layout`: the smallest that fits, and no more than
/// twice the size, so a small request does not pin a large block.
unsafe fn take(layout: Layout) -> *mut u8 {
    let kept = &mut *KEPT.0.get();
    if kept.len == 0 {
        return std::ptr::null_mut();
    }
    let want = layout.size();
    let best = (0..kept.len)
        .filter(|&i| {
            let slot = kept.slots[i];
            slot.size >= want && slot.size / 2 <= want && slot.ptr % layout.align() == 0
        })
        .min_by_key(|&i| kept.slots[i].size);
    match best {
        Some(i) => {
            REUSED.fetch_add(1, Ordering::Relaxed);
            kept.remove(i).ptr as *mut u8
        }
        None => std::ptr::null_mut(),
    }
}

/// The event loop is about to park: discard the held-back blocks nothing has
/// reused for [`KEEP_IDLE`].
pub(crate) fn idle() {
    let kept = unsafe { &mut *KEPT.0.get() };
    if kept.len == 0 {
        return;
    }
    let now = Instant::now();
    let mut i = 0;
    while i < kept.len {
        if kept.slots[i]
            .freed
            .is_some_and(|t| now.duration_since(t) >= KEEP_IDLE)
        {
            unsafe { release(kept.remove(i)) };
        } else {
            i += 1;
        }
    }
}

/// Give every held-back block back to malloc: an allocation failed, and
/// they may be what it needs. True when there was any.
unsafe fn release_all() -> bool {
    let kept = &mut *KEPT.0.get();
    let any = kept.len > 0;
    while kept.len > 0 {
        release(kept.remove(kept.len - 1));
    }
    any
}

/// The bytes malloc's block at `ptr` spans for its owner.
unsafe fn usable(ptr: *mut u8) -> usize {
    libc::malloc_usable_size(ptr as *mut libc::c_void)
}

/// Whole pages inside `[address, address + len)`, as `(start, end)`.
fn interior(address: usize, len: usize) -> Option<(usize, usize)> {
    let start = address.checked_add(PAGE - 1)? & !(PAGE - 1);
    let end = address.checked_add(len)? & !(PAGE - 1);
    (end > start).then_some((start, end))
}

unsafe fn request(start: usize, end: usize, source: Source) -> bool {
    if perry_wasm_discard(start as *mut u8, end - start) == 0 {
        STATE.store(OFF, Ordering::Relaxed);
        return false;
    }
    CALLS.fetch_add(1, Ordering::Relaxed);
    DISCARDED.fetch_add((end - start) as u64, Ordering::Relaxed);
    SOURCE_CALLS[source as usize].fetch_add(1, Ordering::Relaxed);
    SOURCE_BYTES[source as usize].fetch_add((end - start) as u64, Ordering::Relaxed);
    true
}

/// Discard the whole pages inside `[data, data + len)`, whose contents the
/// caller no longer needs. Returns the bytes the host released.
pub(crate) unsafe fn discard_interior(data: *mut u8, len: usize, source: Source) -> usize {
    discard_addresses(data as usize, len, source)
}

/// [`discard_interior`] for a range given by address, for memory that is no
/// longer a live allocation (no pointer into it may be formed).
unsafe fn discard_addresses(address: usize, len: usize, source: Source) -> usize {
    match interior(address, len) {
        Some((start, end)) if enabled() && request(start, end, source) => end - start,
        _ => 0,
    }
}

/// Discard `[data, data + len)` and report whether all of it now reads as
/// zero: the whole pages by the host, the partial edge pages by hand.
pub(crate) unsafe fn discard_zeroed(data: *mut u8, len: usize) -> bool {
    let begin = data as usize;
    let Some((start, end)) = interior(begin, len) else {
        return false;
    };
    if !enabled() || !request(start, end, Source::Buffer) {
        return false;
    }
    std::ptr::write_bytes(data, 0, start - begin);
    std::ptr::write_bytes(end as *mut u8, 0, begin + len - end);
    true
}

/// ` mem=<linear memory bytes> discarded=<bytes released so far>
/// discards=<calls> kept=<bytes held back now> reused=<held-back blocks handed
/// out again> zeroed_reused=<of those, cleared for a zeroed allocation>
/// by=<source>:<calls>/<KiB>,...` for the `PERRY_GC_LOG` line.
pub(crate) fn log_suffix() -> String {
    let memory = core::arch::wasm32::memory_size(0) as u64 * 65536;
    let kept = unsafe { (*KEPT.0.get()).bytes };
    let by = SOURCES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            format!(
                "{name}:{}/{}",
                SOURCE_CALLS[i].load(Ordering::Relaxed),
                SOURCE_BYTES[i].load(Ordering::Relaxed) >> 10
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        " mem={memory} discarded={} discards={} kept={kept} reused={} zeroed_reused={} by={by}",
        DISCARDED.load(Ordering::Relaxed),
        CALLS.load(Ordering::Relaxed),
        REUSED.load(Ordering::Relaxed),
        REUSED_ZEROED.load(Ordering::Relaxed)
    )
}

/// The runtime's global allocator on WASI: wasi-libc's malloc, which never
/// returns memory, plus a held-back list of large freed blocks whose pages
/// go back to the host once they stop being reused.
pub struct DiscardAlloc;

unsafe impl GlobalAlloc for DiscardAlloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() >= MIN_FREE_DISCARD {
            let ptr = take(layout);
            if !ptr.is_null() {
                return ptr;
            }
        }
        let ptr = System.alloc(layout);
        if ptr.is_null() && release_all() {
            return System.alloc(layout);
        }
        ptr
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.size() >= MIN_FREE_DISCARD {
            let ptr = take(layout);
            if !ptr.is_null() {
                std::ptr::write_bytes(ptr, 0, layout.size());
                REUSED_ZEROED.fetch_add(1, Ordering::Relaxed);
                return ptr;
            }
        }
        let ptr = System.alloc_zeroed(layout);
        if ptr.is_null() && release_all() {
            return System.alloc_zeroed(layout);
        }
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.size() >= MIN_FREE_DISCARD {
            if keep(ptr) {
                return;
            }
            // Before the free: malloc writes its free-chunk links afterwards,
            // into whatever the pages read then. The whole block, which can be
            // larger than `layout` when it came off the keep list.
            discard_interior(ptr, usable(ptr), Source::Free);
        }
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        // Growing into a held-back block: copy, and hold the old one back in
        // turn, rather than have malloc free it (and this discard it) on a move.
        if new_size > old && new_size >= MIN_FREE_DISCARD {
            let new = take(Layout::from_size_align_unchecked(new_size, layout.align()));
            if !new.is_null() {
                std::ptr::copy_nonoverlapping(ptr, new, old);
                self.dealloc(ptr, layout);
                return new;
            }
        }
        // What malloc gives up is measured on the block, not the layout: one
        // off the keep list can be up to twice the size it was asked for.
        let block = usable(ptr);
        let old_addr = ptr as usize;
        let mut new = System.realloc(ptr, layout, new_size);
        if new.is_null() && release_all() {
            new = System.realloc(ptr, layout, new_size);
        }
        // Only this thread allocates in this memory (wasm32-wasip1 has one),
        // so what realloc gave up is still free when it returns. Its
        // allocator metadata sits within FREE_MARGIN of either end.
        if !new.is_null() && block >= MIN_FREE_DISCARD {
            let margin = 2 * FREE_MARGIN;
            if new != ptr {
                discard_addresses(
                    old_addr.saturating_add(FREE_MARGIN),
                    block.saturating_sub(margin),
                    Source::Free,
                );
            } else if block.saturating_sub(new_size) >= MIN_FREE_DISCARD {
                discard_addresses(
                    old_addr
                        .saturating_add(new_size)
                        .saturating_add(FREE_MARGIN),
                    block.saturating_sub(new_size).saturating_sub(margin),
                    Source::Shrink,
                );
            }
        }
        new
    }
}
