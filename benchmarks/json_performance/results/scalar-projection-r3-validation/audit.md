# R3 materialized read audit

Source 2647ea7f0 (following 99625043), based on measured R2 with a test-only string-accessor correction.

The original receiver is proved tagged, in the managed address band, integer-indexed and unforwarded by dynamic IndexGet before the optional JSON edge. Ordinary Array/Object dispatch remains ahead of JSON. Lazy brand 9, magic LZXA, no descriptor flags, and nonnull materialized edge admit the backing header inspection. That edge is managed by the existing runtime descriptor. Array brand 1 and an unset growth-forwarded bit are required before any Array payload load. No helper call or GC poll occurs during the borrowed-pointer lifetime.

An admitted backing array refreshes LazyArrayHeader.cached_length (u32, pointer-free), matching resolve_materialized_array. The shared Array guard phi selects either the original ordinary array or the materialized backing array. Reserved flags, length, capacity and elements all use this selected handle. Descriptor state, global prototype invalidation, bounds and length/capacity checks remain authoritative. Hole conversion and numeric coercion retain existing behavior. The fallback receives the original boxed lazy receiver so it can resolve a growth-forwarded target and update the owner's managed edge and cached length normally.

No layout change versus R2, extra allocation, scalar-memo rule change, producer change, new collector call, new root holder or GC policy change. Non-projection sites emit no new Array phi or predecessor.

The fixture holds grownAlias before pushes, preserving the original lazy wrapper if the compiler rewrites the grown local after push. It reads through the alias after every push and checks length, identity, a prototype-filled deleted index, and shrink. Existing descriptor/getter/mutation/order/retention/stringify checks remain in the same fixture.

Main native proof: the identical saved 28-line R2 fixture, saved checker and production statepoint passes reproduce both previously reported hazards on the freshly built main 53df compiler. Main emits zero scalar probes; R2 emits 49. Both have identical two fingerprints, 790 statepoints, 601 live bundles, 3559 relocates. These are inherited, unsuppressed findings, not a clean checker verdict.
