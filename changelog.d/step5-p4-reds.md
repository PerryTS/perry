Runtime housekeeping for charter step 5 (P4): `Object.assign` moves from
`object/alloc.rs` to `object/assign.rs` and the collector's child-slot views
from `gc/layout.rs` to `gc/layout/child_slots.rs` (pure relocations, both files
back under the size gate); the debug-only box remembered-set re-derivation is
compiled only where it is used; a test pins that old-gen defrag never selects a
Longlived page (module class keys arrays live there and workers copy their
addresses).
