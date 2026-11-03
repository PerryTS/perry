Removed cross-module dispatch and argument-bundle overhead on the ECS command
path. Fourteen general compiler/runtime mechanisms: safe cross-module
free-function graph inlining, resolved Array header reuse across indexed
stores, split dynamic canonical Array read keys, guarded direct calls that
involve synthesized `arguments`, scalarized length-only `arguments` bundles,
direct captureless `Array.some` callbacks, inlined bounded tiny allocation
kernels and function-candidate optimization inside candidate methods,
preserved Map-entry types inside function bodies, trusted validated rooted
iterator headers, on-demand Array element-shape proofs, reused dynamic
all-pointer append proofs, inlined runtime-branded `Map.size`/`Set.size`
reads, and fully inlined equality against exact three-byte string literals.
On the upstream `codehz/ecs` "5k entities: 3 commands each + sync" row the
retained control moved from 8.610 ms to 7.287 ms per operation on the pinned
M1 Mac mini (15/15 paired wins, 30/30 semantic oracles).
