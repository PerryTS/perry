use super::*;

/// Type ID constant for Buffer/Uint8Array - matches class_id 0xFFFF0004
pub const BUFFER_TYPE_ID: u32 = 0xFFFF0004;

/// Buffer header - similar to StringHeader but specifically for binary data
/// NOTE: Layout must match ArrayHeader (length at offset 0, capacity at offset 4)
/// because the codegen treats Uint8Array like arrays with hardcoded offsets.
#[repr(C)]
pub struct BufferHeader {
    /// Length in bytes
    pub length: u32,
    /// Capacity (allocated space)
    pub capacity: u32,
}

/// Calculate the layout for a buffer with given capacity
fn buffer_layout(capacity: usize) -> Layout {
    let total_size = std::mem::size_of::<BufferHeader>() + capacity;
    Layout::from_size_align(total_size, 8).unwrap()
}

#[inline]
fn buffer_payload_size(capacity: usize) -> usize {
    std::mem::size_of::<BufferHeader>() + capacity
}

#[inline]
fn buffer_gc_total_size(capacity: usize) -> usize {
    let payload = buffer_payload_size(capacity);
    (crate::gc::GC_HEADER_SIZE + payload + 7) & !7
}

/// Thread-local registry of buffer pointers for instanceof checks.
/// Since BufferHeader has the same layout as ArrayHeader (no type_id field),
/// we track buffer pointers separately to distinguish them from arrays.
use crate::fast_hash::{new_ptr_hash_map, new_ptr_hash_set, PtrHashMap, PtrHashSet};
use std::cell::RefCell;

thread_local! {
    static BUFFER_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
    /// Buffers that were specifically created via `new Uint8Array(...)` —
    /// formatted as `Uint8Array(N) [ a, b, c ]` instead of `<Buffer aa bb cc>`.
    static UINT8ARRAY_FROM_CTOR: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
    /// Issue #579: buffers allocated as `new ArrayBuffer(n)` — sources that
    /// `new Uint8Array(ab)` should ALIAS rather than copy. Survives across
    /// `mark_as_uint8array` calls so a second view of the same ArrayBuffer
    /// still aliases (without a separate registry, the first view's mark
    /// would make the second `js_uint8array_new` call mistake the source
    /// for a Uint8Array and fall into the spec-mandated COPY branch).
    static ARRAY_BUFFER_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
    /// Issue #1225: ArrayBuffer-identity alias map for Buffers produced by
    /// copy paths like `Buffer.from(buf)`.  Node-compatible semantics: the
    /// new Buffer's `.buffer` returns the same ArrayBuffer object as the
    /// source's `.buffer` because both views live inside the shared 8 KiB
    /// pool slab.  Perry allocates fresh inline storage per Buffer, so the
    /// `.buffer` getter would otherwise return the new BufferHeader pointer
    /// and `src.buffer === cp.buffer` would be false.  Storing the source's
    /// resolved alias here lets the getter return a stable identity token.
    /// Limitation: the bytes are not actually inside the aliased buffer, so
    /// reads/writes through `.buffer` won't observe the view's data — only
    /// the `===` identity check matches Node.
    static BUFFER_AB_ALIAS: RefCell<PtrHashMap<usize, usize>> =
        RefCell::new(new_ptr_hash_map());
}

pub fn mark_as_array_buffer(addr: usize) {
    ARRAY_BUFFER_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr);
    });
}

pub fn is_array_buffer(addr: usize) -> bool {
    ARRAY_BUFFER_REGISTRY.with(|r| r.borrow().contains(&addr))
}

/// Register a buffer pointer in the thread-local registry
pub fn register_buffer(ptr: *const BufferHeader) {
    BUFFER_REGISTRY.with(|r| r.borrow_mut().insert(ptr as usize));
}

// ----- Small-buffer slab allocator ----------------------------------------
//
// GC interaction:
//   Buffers carry no GcHeader and are not tracked in MALLOC_STATE (the existing
//   malloc path also never calls `dealloc` on individual buffers — they live for
//   the lifetime of the thread). Slab blocks are malloc'd once and retained for
//   the same duration. No GC behaviour changes.
//
// Registry:
//   Large buffers (capacity >= SMALL_BUF_THRESHOLD) still go through
//   `register_buffer` and appear in BUFFER_REGISTRY (HashSet).
//   Small buffers skip the HashSet insert; `is_registered_buffer` instead
//   performs a range-check against the (tiny) list of slab blocks — O(n_slabs),
//   typically ≤ 5 entries for a 100k-iteration allocation loop.
//   No false positives: slab blocks exclusively contain BufferHeader allocations
//   and all callers of `is_registered_buffer` pass the header pointer (the
//   NaN-boxed POINTER_TAG value always points to the header start, never to
//   interior data bytes).

/// Capacities strictly below this threshold use the slab fast path.
pub const SMALL_BUF_THRESHOLD: u32 = 256;

/// One slab block covers this many bytes of BufferHeader+data storage.
/// 256 KB → ≥ 1 000 allocations of the max small size (255 bytes), or up to
/// 32 768 allocations of the minimum (0 bytes / header only).
const SLAB_CAPACITY: usize = 256 * 1024;

/// Per-thread bump-pointer slab for small buffers.
/// Raw pointers stored as `usize` to keep the type `Send + Sync`.
struct SmallBufSlab {
    /// Byte offset of the next free slot within the current slab block.
    current: usize,
    /// One-past-the-end offset (absolute address as usize) of the current block.
    end: usize,
    /// (start, end) address pair for every slab block allocated so far.
    /// Used by `is_registered_buffer` to confirm an address is a small buffer.
    ranges: Vec<(usize, usize)>,
}

thread_local! {
    static SMALL_BUF_SLAB: RefCell<SmallBufSlab> = const { RefCell::new(SmallBufSlab {
        current: 0,
        end: 0,
        ranges: Vec::new(),
    }) };
}

fn buffer_alloc_small(capacity: u32) -> *mut BufferHeader {
    let needed = std::mem::size_of::<BufferHeader>() + capacity as usize;
    // Round up to 8-byte boundary so every header is naturally aligned.
    let aligned = (needed + 7) & !7;

    SMALL_BUF_SLAB.with(|slab_ref| {
        let mut slab = slab_ref.borrow_mut();

        if slab.current + aligned > slab.end {
            // Current block exhausted (or first call): allocate a fresh slab.
            let layout = Layout::from_size_align(SLAB_CAPACITY, 8).unwrap();
            let block = unsafe { alloc(layout) };
            if block.is_null() {
                panic!(
                    "buffer: failed to allocate small-buffer slab ({} bytes)",
                    SLAB_CAPACITY
                );
            }
            let block_start = block as usize;
            let block_end = block_start + SLAB_CAPACITY;
            slab.ranges.push((block_start, block_end));
            slab.current = block_start;
            slab.end = block_end;
        }

        let ptr = slab.current as *mut BufferHeader;
        slab.current += aligned;

        unsafe {
            (*ptr).length = 0;
            (*ptr).capacity = capacity;
        }

        ptr
    })
}

/// Check if a pointer is a registered buffer (for instanceof Uint8Array)
pub fn is_registered_buffer(addr: usize) -> bool {
    // Fast path: address falls within a small-buffer slab block.  All bytes in
    // a slab block belong exclusively to BufferHeader allocations, so any match
    // is definitively a buffer pointer.
    let in_slab = SMALL_BUF_SLAB.with(|slab_ref| {
        let slab = slab_ref.borrow();
        slab.ranges
            .iter()
            .any(|&(start, end)| addr >= start && addr < end)
    });
    if in_slab {
        return true;
    }
    // Slow path: large buffers tracked in the HashSet registry.
    BUFFER_REGISTRY.with(|r| r.borrow().contains(&addr))
}

/// Mark this buffer as one that came from `new Uint8Array(...)` so it
/// formats as `Uint8Array(N) [ ... ]` rather than `<Buffer ...>`.
pub fn mark_as_uint8array(addr: usize) {
    UINT8ARRAY_FROM_CTOR.with(|r| {
        r.borrow_mut().insert(addr);
    });
}

pub fn is_uint8array_buffer(addr: usize) -> bool {
    UINT8ARRAY_FROM_CTOR.with(|r| r.borrow().contains(&addr))
}

/// Record that `buf`'s `.buffer` property should resolve to `alias` instead of
/// `buf` itself.  Used by copy paths (`Buffer.from(src)`) to propagate the
/// source's ArrayBuffer identity onto the new buffer — see #1225.
pub fn set_buffer_ab_alias(buf: usize, alias: usize) {
    BUFFER_AB_ALIAS.with(|m| {
        m.borrow_mut().insert(buf, alias);
    });
}

/// Look up the ArrayBuffer-identity alias for a Buffer.  Returns `None` for
/// buffers that haven't been involved in a copy chain (their `.buffer` just
/// returns themselves, as before).
pub fn buffer_ab_alias(buf: usize) -> Option<usize> {
    BUFFER_AB_ALIAS.with(|m| m.borrow().get(&buf).copied())
}

/// Collapse an alias chain to its root: if `buf` already aliases something,
/// return that; otherwise return `buf` itself.  Callers use this to seed the
/// alias on a fresh copy so chained `Buffer.from(Buffer.from(src))` keeps
/// `===` identity with the original source.
pub fn resolve_buffer_ab_alias(buf: usize) -> usize {
    buffer_ab_alias(buf).unwrap_or(buf)
}

/// Allocate a buffer with the given capacity
pub fn buffer_alloc(capacity: u32) -> *mut BufferHeader {
    // Fast path: small buffers come from a per-thread bump slab (no malloc,
    // no HashSet insert).  Large buffers fall through to the existing malloc path.
    if capacity < SMALL_BUF_THRESHOLD {
        return buffer_alloc_small(capacity);
    }
    if crate::gc::is_large_object_total_size(buffer_gc_total_size(capacity as usize)) {
        let ptr = crate::arena::arena_alloc_gc_old(
            buffer_payload_size(capacity as usize),
            8,
            crate::gc::GC_TYPE_BUFFER,
        ) as *mut BufferHeader;
        unsafe {
            let header =
                (ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*header).gc_flags |= crate::gc::GC_FLAG_TENURED;
            (*ptr).length = 0;
            (*ptr).capacity = capacity;
        }
        register_buffer(ptr);
        return ptr;
    }
    let layout = buffer_layout(capacity as usize);
    unsafe {
        let ptr = alloc(layout) as *mut BufferHeader;
        if ptr.is_null() {
            panic!("Failed to allocate buffer");
        }
        (*ptr).length = 0;
        (*ptr).capacity = capacity;
        register_buffer(ptr);
        ptr
    }
}

/// Get the data pointer for a buffer
pub fn buffer_data(buf: *const BufferHeader) -> *const u8 {
    unsafe { (buf as *const u8).add(std::mem::size_of::<BufferHeader>()) }
}

/// Get the mutable data pointer for a buffer
pub fn buffer_data_mut(buf: *mut BufferHeader) -> *mut u8 {
    unsafe { (buf as *mut u8).add(std::mem::size_of::<BufferHeader>()) }
}
