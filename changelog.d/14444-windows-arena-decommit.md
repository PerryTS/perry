Windows: GC arena blocks are now their own `VirtualAlloc` reservations instead
of global-allocator memory. A pooled empty block whose reuse window expires is
decommitted (`MEM_DECOMMIT`), so its commit charge is returned to the system,
and it is committed again when the pool hands it out. Before, the pool's
release was a no-op on Windows: pooled blocks stayed committed and block churn
fragmented the allocator's arenas, so long-running programs kept growing.
