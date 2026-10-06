//! Process-global `SharedArrayBuffer` backing store + registry (#4913 Stage 2).
//!
//! A `SharedArrayBuffer` is the one JavaScript value whose bytes must alias the
//! same physical memory across every `perry/thread` agent. Ordinary buffers are
//! thread-local slab / arena allocations whose addresses are only meaningful on
//! the owning thread, and crossing a thread boundary deep-copies them — so they
//! cannot back cross-agent `Atomics` coordination.
//!
//! SAB backing is therefore allocated directly from the global allocator,
//! never freed (matching Perry's "buffers live for the life of the process"
//! model — see `buffer::header`), and recorded in a process-global registry so
//! any thread can:
//!   * recognise a raw pointer as a shared backing store (during cross-thread
//!     serialization, before the missing `GcHeader` would be misread), and
//!   * keep the backing alive for the life of the process.
//!
//! Its brand is in its own `GcHeader` (`GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER`,
//! #10694), so every thread recognises it as a `SharedArrayBuffer` without
//! registering it anywhere.
//!
//! Because the address is a stable, process-wide heap address, an `Atomics`
//! slot inside a SAB has the same absolute byte address on every thread — which
//! is exactly the key the futex wait/notify table ([`crate::atomics_futex`])
//! uses to match a `notify` on one agent with a `wait` parked on another.

use std::alloc::{alloc_zeroed, handle_alloc_error, Layout};
use std::collections::HashSet;

std::thread_local! {
    // Shared backings are never freed today. Attribute each allocation to its
    // creator, once; receiving a shared alias contributes no new bytes.
    static BACKING_BYTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(crate) fn current_thread_backing_bytes() -> u64 {
    BACKING_BYTES.with(std::cell::Cell::get)
}
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::buffer::BufferHeader;
// `GC_FLAG_PINNED` is deliberately NOT imported: the #7645 custody gate reads a
// bare mention of the token as a pin creation, and this module only ever masks
// with it (in the header-survival test). Spelled in full at those two reads.
use crate::gc::{GcHeader, GC_FLAG_TENURED, GC_HEADER_SIZE, GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER};

/// Set of `BufferHeader` addresses that back a `SharedArrayBuffer`.
static SHARED_SAB_REGISTRY: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

/// Latched true by the first SAB allocation, so a process that never allocates
/// a `SharedArrayBuffer` answers [`is_shared_sab`] from one atomic load without
/// touching the lock.
static SHARED_SAB_NONEMPTY: AtomicBool = AtomicBool::new(false);

fn registry() -> &'static Mutex<HashSet<usize>> {
    crate::once_init::get_or_init(&SHARED_SAB_REGISTRY, || Mutex::new(HashSet::new()))
}

/// Layout for a SAB of `size` data bytes:
/// `[GcHeader:8][BufferHeader:8][data:size]`, 8-byte aligned.
///
/// #340/#341 / #10925: the leading `GcHeader` is what makes a SAB an honest
/// pointer. Before it, the JS value (the `BufferHeader` address) had no header,
/// and every `*(addr - 8)` type probe read whatever `alloc_zeroed` block sat in
/// front of it — the tail of another SAB's user-writable data — so writing a
/// SAB's own bytes could flip `Array.isArray` on another and crash a brand
/// check (#10925). With the header, `BufferHeader` and the data region keep
/// their exact offsets (the returned pointer still points at the `BufferHeader`,
/// so `buffer_data` == `buf + 8` is unchanged), and `buf - 8` is a real
/// `GC_TYPE_BUFFER` header. 8-byte alignment keeps the data region 8-aligned for
/// `BigInt64Array` / `Float64` atomic slots.
fn sab_layout(size: u32) -> Layout {
    let total = GC_HEADER_SIZE + crate::codegen_abi::BYTES_STORE + size as usize;
    Layout::from_size_align(total, 8).expect("shared SAB layout")
}

/// Allocate a process-global, never-freed `BufferHeader + size` block for a
/// `SharedArrayBuffer`. The returned address is stable for the life of the
/// process and valid (readable / writable) from every thread, so views built
/// over it on different agents alias the same physical bytes.
fn alloc_shared_block(size: u32) -> *mut BufferHeader {
    // RULE 3 (`object/shape_rule3.rs`): a SAB's bytes are `alloc_zeroed`, so
    // unlike an arena buffer a 2 GiB request really can succeed — and it would
    // write `0x8000_0000` into `capacity` at payload `+4`, which the emitted
    // read path cannot tell from shape #1. Same ceiling and same `RangeError`
    // as `new ArrayBuffer(n)`.
    let size =
        crate::object::shape_rule3::checked_plus_four_word(size, b"Array buffer allocation failed");
    let layout = sab_layout(size);
    // SAFETY: `layout` has non-zero size (BufferHeader is 8 bytes) and 8-byte
    // alignment. `alloc_zeroed` gives the spec-required zero-initialized bytes.
    let raw = unsafe { alloc_zeroed(layout) };
    if raw.is_null() {
        handle_alloc_error(layout);
    }
    BACKING_BYTES.with(|bytes| bytes.set(bytes.get().saturating_add(size as u64)));
    // The `GcHeader` sits at `raw`; the JS-visible value is the `BufferHeader`
    // one header down, so `buf - GC_HEADER_SIZE` reads back this header.
    let buf = unsafe { raw.add(GC_HEADER_SIZE) } as *mut BufferHeader;
    let total = layout.size();
    // SAFETY: `raw` owns `total` zeroed, 8-aligned bytes; the header and the
    // BufferHeader both fit within the first `GC_HEADER_SIZE + 8` of them.
    unsafe {
        let header = raw as *mut GcHeader;
        (*header).obj_type = GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER;
        // PINNED + TENURED and NOT `GC_FLAG_ARENA`: this block is a raw,
        // process-global `alloc_zeroed`, not an arena or a gc_malloc cell. The
        // collector recognises a SAB by process-global registry membership
        // (`is_shared_sab`), never by this header, and — proven by the
        // header-write audit in the PR — no collector path (mark, scavenge,
        // sweep, remembered-set) reaches an object outside its own thread's
        // arena/tracked set, so this header is only ever READ by the collector,
        // never written. It carries the honest kind for the mutator-side
        // `*(addr - 8)` probes (`Array.isArray`, the collection-thunk brand,
        // `JSON.stringify`), which is what #10925 needed.
        (*header).gc_flags = GC_FLAG_TENURED;
        (*header)._reserved = 0;
        // Total block size, for honesty; a non-arena object is never block-walked.
        (*header).size = total.min(u32::MAX as usize) as u32;
        std::ptr::write(
            buf,
            BufferHeader {
                length: size,
                capacity: size,
                link: 0,
            },
        );
        // #7645 custody: the PIN goes through `gc::pin`, not a raw flag write.
        // `pin_object_non_young` is the right variant and its safety contract
        // is met by construction — this block is a process-global
        // `alloc_zeroed` with no `GC_FLAG_ARENA`, so it is malloc space and can
        // never be Eden/FromSurvivor, and the latch must stay disarmed for it.
        // `pin_object_non_young_call_sites_are_never_young` carries the case.
        crate::gc::pin_object_non_young(header);
    }
    // Latch BEFORE the insert, not after, so no window exists in which the
    // entry is live and `is_shared_sab` still takes the idle fast path (see
    // `crate::registry_latch`).
    SHARED_SAB_NONEMPTY.store(true, Ordering::Release);
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(buf as usize);
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(crate::hot_diag::ReceiverReprFamily::Sab);
    }
    buf
}

/// Each agent owns its metadata while sharing only the process store.
pub fn alloc_shared_sab(size: u32) -> *mut BufferHeader {
    let block = alloc_shared_block(size);
    wrap_shared_sab(block as usize)
}

pub(crate) fn wrap_shared_sab(block: usize) -> *mut BufferHeader {
    assert!(registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&block));
    unsafe {
        let owner = crate::buffer::header::buffer_alloc_foreign(
            crate::buffer::store::owner_data(block),
            (*(block as *const BufferHeader)).length,
        );
        crate::buffer::mark_as_shared_array_buffer(owner as usize);
        owner
    }
}

/// Proven process capability, never an agent's own mutable cell address.
pub(crate) fn shared_store_owner(addr: usize) -> Option<usize> {
    if !SHARED_SAB_NONEMPTY.load(Ordering::Acquire) {
        return None;
    }
    if registry()
        .lock()
        .map(|r| r.contains(&addr))
        .unwrap_or(false)
    {
        return Some(addr);
    }
    unsafe {
        let h = crate::value::addr_class::try_read_tracked_gc_header(addr)?.as_ref();
        if h.obj_type != GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER
            || h._reserved & crate::codegen_abi::BYTES_OUT_OF_LINE == 0
        {
            return None;
        }
        let block = (crate::buffer::store::owner_data(addr) as usize)
            .checked_sub(crate::codegen_abi::BYTES_STORE)?;
        registry().lock().ok()?.contains(&block).then_some(block)
    }
}

/// True if `addr` is a process-global `SharedArrayBuffer` backing store, as
/// opposed to a thread-local SharedArrayBuffer copy (`slice`, structuredClone)
/// that carries the same brand: the cross-thread serializer passes the former
/// by reference and must deep-copy the latter.
pub(crate) fn is_shared_block(addr: usize) -> bool {
    SHARED_SAB_NONEMPTY.load(Ordering::Acquire)
        && registry()
            .lock()
            .map(|r| r.contains(&addr))
            .unwrap_or(false)
}

pub fn is_shared_sab(addr: usize) -> bool {
    shared_store_owner(addr).is_some()
}

#[cfg(test)]
mod header_survival_tests {
    use super::*;

    /// #10925, the precondition for putting a `GcHeader` in front of
    /// process-global memory: **no collector may WRITE it.** Two threads'
    /// collectors setting a mark or forwarding bit on one header would be a
    /// data race that shows up as rare corruption rather than a clean failure.
    ///
    /// The argument is the source audit (plan L15.7): every mark, scavenge,
    /// sweep and remembered-set write gates on THIS thread's arena or
    /// malloc-tracked membership — a set a process-global SAB is in on no
    /// thread — and the moving paths classify by arena range before they read
    /// a header at all. This test is the empirical backstop for that argument,
    /// not a proof of it: it snapshots the header word, drives several minor
    /// and major collections on this thread AND on two others while all three
    /// hold the SAB, and requires the word to come back unchanged.
    ///
    /// It can fail: point `alloc_shared_sab` at the arena, or drop the
    /// membership gate in front of any mark write, and the mark bit lands in
    /// this word.
    #[test]
    fn no_collector_writes_a_shared_sab_header() {
        let buf = alloc_shared_block(64);
        let header_addr = (buf as usize) - GC_HEADER_SIZE;
        // Read as one 64-bit word: obj_type, gc_flags, _reserved and size
        // together, so a write to ANY of them is caught.
        let snapshot = unsafe { std::ptr::read_volatile(header_addr as *const u64) };

        // The header must actually say what the fix intends, or "unchanged"
        // would be vacuous.
        // The canonical read predicate, not a bare cast: this is an ordinary
        // header READ and `try_read_gc_header` expresses it exactly, the same
        // way `object::tombstone_tests` reads a keys array's flags.
        // `try_read_gc_header` takes the OBJECT address and reads the header at
        // `addr - GC_HEADER_SIZE`; `header_addr` is already that subtraction,
        // so passing it reads a header's-worth of bytes too far back.
        let header = unsafe { crate::value::addr_class::try_read_gc_header(buf as usize) }
            .expect("the shared SAB block carries a GcHeader");
        assert_eq!(header.obj_type, GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER);
        // Masking reads, per the #7645 custody gate: a creation of the flag
        // may only live in `gc/pin.rs`. Both bits set, and nothing else.
        assert_ne!(
            header.gc_flags & crate::gc::GC_FLAG_PINNED,
            0,
            "the SAB header is pinned"
        );
        assert_ne!(
            header.gc_flags & GC_FLAG_TENURED,
            0,
            "the SAB header is tenured"
        );
        assert_eq!(
            header.gc_flags & !(crate::gc::GC_FLAG_PINNED | GC_FLAG_TENURED),
            0,
            "no other flag is set on a SAB header"
        );

        fn churn() {
            for _ in 0..8 {
                for _ in 0..2000 {
                    let o = crate::object::js_object_alloc(0, 0);
                    std::hint::black_box(o);
                }
                crate::gc::js_gc_collect();
            }
        }

        let workers: Vec<_> = (0..2)
            .map(|_| {
                let addr = buf as usize;
                std::thread::spawn(move || {
                    // Touch the shared bytes the way an Atomics user would,
                    // so the SAB is live across this thread's collections.
                    crate::buffer::bytes::no_gc(|_| {
                        let data = unsafe { crate::buffer::store::owner_data(addr) };
                        for i in 0..64u8 {
                            unsafe {
                                (*(data.add(i as usize) as *const std::sync::atomic::AtomicU8))
                                    .store(i, Ordering::Relaxed)
                            };
                        }
                    });
                    churn();
                })
            })
            .collect();
        churn();
        for w in workers {
            w.join().expect("a collecting thread panicked");
        }

        let after = unsafe { std::ptr::read_volatile(header_addr as *const u64) };
        assert_eq!(
            after, snapshot,
            "a collector wrote the process-global SAB header: {snapshot:#018x} -> {after:#018x}"
        );
    }
}
