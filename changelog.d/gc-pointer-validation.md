### Reduce GC pointer-validation and Buffer probe overhead

The synchronous collector now rebuilds each arena block's exact-start bitmap
from its own object census and uses the direct page-class metadata for constant-
time `ValidPointerSet` membership. This covers codegen's inline allocator
without adding work to the allocation path; the existing sorted census runs
remain the kill-switch fallback and continue to serve interior-pointer lookup.

Generic Buffer identity checks now reject tracked non-Buffer GC objects from
their header class before consulting the address filter and exact side
registry. Positive matches and headerless external Buffer/SharedArrayBuffer
storage still use the authoritative registries.

`PERRY_GC_DIAG` now reports pointer-membership and runtime-handle scope/push
counts, and `PERRY_BUFFER_DIAG` reports header-class rejections, so the cc turn
rig can attribute the affected hot paths directly.
