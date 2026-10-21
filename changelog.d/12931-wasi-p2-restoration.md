Restore standalone WASI P2 command builds and execution after runtime ABI,
object metadata and socket ownership changes.

Register runtime image atoms/layouts through Wasm constructors in linear
memory, define their ILP32 layouts and retain the published BandDir offsets.
Adapt the WASI socket driver to the current owner-link transport contract,
including pinned queued completions, accepted-socket ownership and stale
incarnation/deadline rejection. Replace incorrect internal array FFI
redeclarations with typed Rust calls.

Refresh the generated runtime ABI table and teach its checker to read closure
entry macros and receiver-specialized entries. Preserve exception stack marks
and distinguish 32-bit closure pointers from 64-bit receiver values. Include
the selected runtime feature installer in WASI links, restoring normalization
and URL prototype accessors. Add moving-GC/static-metadata and socket-reuse
acceptance cases.

Validation on Linux x86_64 (Wasmtime 48, wasi-sdk 34, LLVM 22,
nightly-2026-10-04): 6/6 smoke, 20/20 acceptance, 10/10 comparisons against
Node 26.5.1, 9/9 ABI/closure lowering tests, 16/16 focused native regressions
and 6/6 harness tests. Both WASI runtime feature configurations, runtime ABI
table, native ABI and Node pin checks pass. Fix the ABI checker's false
positive on repeated parameter types. File-size, address and GC-holder
inventory gates have existing baseline failures; Windows/macOS reruns and
full CI are outstanding.

Document the actual output/hosting contracts and remaining P1 hosting,
typed P2 component and P3 async milestones. This change does not implement
those hosting/plugin APIs.
