//! Per-agent Buffer pool. Placement policy is kept behind one call for B3.
use super::BufferHeader;
use std::cell::Cell;

crate::perry_thread_local! {
    static POOL_OWNER: Cell<usize> = const { Cell::new(0) };
    static POOL_OFFSET: Cell<u32> = const { Cell::new(0) };
    // Where the pool starts inside its ArrayBuffer, and the `Buffer.poolSize`
    // it was created with: node's `poolBase` and `poolSize`.
    static POOL_BASE: Cell<u32> = const { Cell::new(0) };
    static POOL_SIZE: Cell<u32> = const { Cell::new(0) };
}

/// node's `kPoolAlignment`: the pool is aligned to a cache line.
const POOL_ALIGNMENT: u32 = 64;

/// node's `alignmentPadding(alignment)` (26.12, `buffer: pad aligned
/// allocations by a multiple of 8`): `createUnsafeAlignedBuffer(size,
/// alignment)` allocates `size + max(alignment, 8)` bytes and starts the
/// buffer at the aligned offset, so the bytes after it stay a whole number of
/// 8-byte elements.
const fn alignment_padding(alignment: u32) -> u32 {
    if alignment > 8 {
        alignment
    } else {
        8
    }
}

pub(crate) use super::store::Init;

pub(crate) fn place(brand: u8, init: Init<'_>, len: u32) -> *mut BufferHeader {
    super::store::store_alloc(brand, len, init)
}

/// PoolView is a mechanism; eligibility and the new pool size come from policy.
pub(crate) fn alloc_view(size: u32, len: u32) -> *mut BufferHeader {
    // Construction does not collect with an unrooted source byte span live.
    let _suppress = crate::gc::GcSuppressScope::new();
    POOL_OWNER.with(|owner| {
        POOL_OFFSET.with(|offset| unsafe {
            let mut pool = owner.get();
            let start = offset.get();
            // node's `allocate()`: `if (size > poolSize - poolOffset) createPool()`.
            if pool == 0 || start.saturating_add(len) > POOL_SIZE.with(Cell::get) {
                // node's `createPool()`: `createUnsafeAlignedBuffer(poolSize,
                // kPoolAlignment)`.
                let fresh = super::store::store_alloc(
                    crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER,
                    size.saturating_add(alignment_padding(POOL_ALIGNMENT)),
                    Init::Uninit,
                );
                pool = fresh as usize;
                owner.set(pool);
                offset.set(0);
                POOL_SIZE.with(|cell| cell.set(size));
                let addr = super::store::data(pool) as usize;
                let base = (POOL_ALIGNMENT as usize - addr % POOL_ALIGNMENT as usize)
                    % POOL_ALIGNMENT as usize;
                POOL_BASE.with(|cell| cell.set(base as u32));
                crate::gc::runtime_write_barrier_root_nanbox(
                    crate::value::js_nanbox_pointer(pool as i64).to_bits(),
                );
            }
            let start = offset.get();
            let view = super::store::new_view(
                crate::gc::GC_TYPE_BUFFER,
                pool,
                POOL_BASE.with(Cell::get).saturating_add(start),
                len,
                false,
            );
            #[cfg(test)]
            if super::bytes::b4_sabotage("pool_identity") {
                return super::buffer_alloc(len);
            }
            offset.set(start.saturating_add(len).saturating_add(7) & !7);
            view
        })
    })
}

pub(crate) fn copy(len: u32) -> *mut BufferHeader {
    place(crate::gc::GC_TYPE_BUFFER, Init::PoolCopy, len)
}

pub(crate) fn scan_pool_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    POOL_OWNER.with(|owner| {
        let mut addr = owner.get();
        if visitor.visit_usize_slot(&mut addr) {
            owner.set(addr);
        }
    });
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    POOL_OWNER.with(|owner| owner.set(0));
    POOL_OFFSET.with(|offset| offset.set(0));
    POOL_BASE.with(|base| base.set(0));
    POOL_SIZE.with(|size| size.set(0));
}
