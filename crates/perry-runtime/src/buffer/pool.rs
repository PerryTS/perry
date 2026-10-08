//! Per-agent Buffer pool. Placement policy is kept behind one call for B3.
use super::BufferHeader;
use std::cell::Cell;

crate::perry_thread_local! {
    static POOL_OWNER: Cell<usize> = const { Cell::new(0) };
    static POOL_OFFSET: Cell<u32> = const { Cell::new(0) };
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
            if pool == 0
                || start.saturating_add(len) > crate::buffer::store::capacity(pool as usize)
            {
                let fresh = super::store::store_alloc(
                    crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER,
                    size,
                    Init::Uninit,
                );
                pool = fresh as usize;
                owner.set(pool);
                offset.set(0);
                crate::gc::runtime_write_barrier_root_nanbox(
                    crate::value::js_nanbox_pointer(pool as i64).to_bits(),
                );
            }
            let start = offset.get();
            let view = super::store::new_view(crate::gc::GC_TYPE_BUFFER, pool, start, len, false);
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
}
