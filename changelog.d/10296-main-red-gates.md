Clear four of the five CI jobs that were failing on `main`, which every open PR
inherited.

`registered_extern_handle` and `wasm_memory_descriptor_maximum` are read only
from `wasm-host`-gated call sites, so their definitions now carry the same
gate; without it a build without the feature failed `-D warnings` as dead code.
The `thread_local!` #10166 added under `cfg(debug_assertions)` is recorded as
cold rather than converted: it is absent from shipping builds, so it cannot
cost `_tlv_get_addr` there and should not hold a hot-cache slot. The generated
API reference and `.d.ts` are regenerated for the manifest's `bun-pty` entry.

`stack_top_respects_custom_thread_stack_sizes` compared `top - address` against
the REQUESTED stack size, which depends on how far the allocator rounds the
request up and whether the guard page falls inside the region
`pthread_attr_getstack` reports — per-platform, and per-image on a hosted
runner. It failed on Linux CI while holding on this project's own Linux box and
on macOS, where the module is not compiled at all. It now asserts what a stack
walk started on a worker depends on: the bound is that worker's, not the
spawning thread's.

The fifth job, `lint`, needs the published benchmark artifact regenerated: its
`source_fingerprint` no longer matches because `Cargo.toml` changed after the
artifact was generated. That rerun republishes the perry-vs-node numbers, so it
is left to a deliberate refresh.
