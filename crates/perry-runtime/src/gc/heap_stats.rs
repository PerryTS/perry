//! Numeric heap census for `bun:jsc.heapStats`, covering the calling thread.
//!
//! No JS allocations or collection may occur during this walk. Only counters
//! leave the walk, so constructing the JS report afterwards cannot invalidate
//! a saved pointer. As with heap snapshots, uncollected arena residents can
//! appear, but free-list slots and forwarding headers do not.
use super::*;

pub(crate) struct HeapStats {
    pub(crate) arena_used: u64,
    pub(crate) arena_reserved: u64,
    pub(crate) malloc_bytes: u64,
    pub(crate) malloc_count: u64,
    /// Backing bytes, excluding header-only views and detached stores.
    pub(crate) array_buffer_bytes: u64,
    /// Malloc objects plus backing bytes; ArrayBuffers are included once.
    pub(crate) external_bytes: u64,
    pub(crate) object_count: u64,
    pub(crate) pinned_count: u64,
    pub(crate) types: Vec<(&'static str, u64, u64)>,
}

pub(crate) fn heap_stats() -> HeapStats {
    let mut arena_used = 0;
    let mut arena_reserved = 0;
    crate::arena::js_arena_stats(&mut arena_used, &mut arena_reserved);
    let free_slots: std::collections::HashSet<*mut u8> =
        ARENA_FREE_LIST.with(|slots| slots.borrow().iter().map(|&(ptr, _)| ptr).collect());
    let mut counts = [0u64; GC_TYPE_MAX as usize + 1];
    let mut pinned = [0u64; GC_TYPE_MAX as usize + 1];
    let mut malloc_bytes = 0u64;
    let mut malloc_count = 0u64;
    let mut array_buffer_bytes = crate::shared_sab::current_thread_backing_bytes();
    let mut external_bytes = array_buffer_bytes;
    let mut visit = |ptr: *mut u8, malloc: bool| unsafe {
        let header = &*ptr.cast::<GcHeader>();
        if gc_type_info(header.obj_type).is_none()
            || header.size == 0
            || header.gc_flags & GC_FLAG_FORWARDED != 0
            || free_slots.contains(&ptr)
            || free_slots.contains(&ptr.add(GC_HEADER_SIZE))
        {
            return;
        }
        let index = header.obj_type as usize;
        counts[index] += 1;
        if header.gc_flags & GC_FLAG_PINNED != 0 {
            pinned[index] += 1;
        }
        if malloc {
            malloc_count += 1;
            malloc_bytes = malloc_bytes.saturating_add(header.size as u64);
        }
        let user = ptr.add(GC_HEADER_SIZE);
        let (backing, array_buffer) = backing_bytes(header, user);
        array_buffer_bytes = array_buffer_bytes.saturating_add(array_buffer);
        // Malloc's size already includes inline backing. Foreign backing is
        // outside the allocation and must be added separately.
        let inline_backing = if is_buffer_family_type(header.obj_type)
            && header._reserved & GC_BUFFER_FOREIGN_DATA != 0
        {
            0
        } else {
            backing
        };
        external_bytes = external_bytes.saturating_add(if malloc {
            (header.size as u64).saturating_sub(inline_backing) + backing
        } else {
            backing
        });
    };
    crate::arena::arena_walk_objects(|ptr| visit(ptr, false));
    MALLOC_STATE.with(|state| {
        for &header in &state.borrow().objects {
            visit(header.cast(), true);
        }
    });
    HeapStats {
        arena_used,
        arena_reserved,
        malloc_bytes,
        malloc_count,
        array_buffer_bytes,
        external_bytes,
        object_count: counts.iter().sum(),
        pinned_count: pinned.iter().sum(),
        types: gc_type_infos()
            .filter_map(|info| {
                let index = info.type_id as usize;
                (counts[index] != 0).then_some((info.name, counts[index], pinned[index]))
            })
            .collect(),
    }
}

/// Inspect storage ownership, rather than the view's visible byteLength.
/// No allocations or collection are allowed while these pointers are read.
unsafe fn backing_bytes(header: &GcHeader, user: *mut u8) -> (u64, u64) {
    if crate::gc::is_byte_family_type(header.obj_type)
        && !crate::gc::is_byte_view_type(header.obj_type)
        && header._reserved & crate::codegen_abi::BYTES_DETACHED == 0
    {
        let bytes = (*user.cast::<crate::buffer::BufferHeader>()).capacity as u64;
        return (
            bytes,
            if matches!(
                header.obj_type,
                GC_TYPE_BUFFER_SECRET_KEY | GC_TYPE_BUFFER_CRYPTO_KEY
            ) {
                0
            } else {
                bytes
            },
        );
    }
    (0, 0)
}

#[cfg(test)]
mod memory_accounting_tests {
    use super::*;

    #[test]
    fn backing_ownership_excludes_aliases_and_detached_buffers() {
        let before = heap_stats();
        let buf = crate::buffer::buffer_alloc(4 * 1024 * 1024);
        unsafe {
            crate::buffer::store::set_length(buf as usize, (*buf).capacity);
        }
        crate::buffer::mark_as_array_buffer(buf as usize);
        let owned = heap_stats();
        assert_eq!(
            owned.array_buffer_bytes - before.array_buffer_bytes,
            4 * 1024 * 1024
        );
        assert_eq!(
            owned.external_bytes - before.external_bytes,
            4 * 1024 * 1024
        );
        let _alias = crate::buffer::view::alloc_data_view(buf, 0, 32);
        let aliased = heap_stats();
        assert_eq!(aliased.array_buffer_bytes, owned.array_buffer_bytes);
        crate::buffer::detach_array_buffer(buf as usize);
        let detached = heap_stats();
        assert_eq!(detached.array_buffer_bytes, before.array_buffer_bytes);
    }
}
