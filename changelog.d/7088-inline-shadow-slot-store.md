**Perf (GC): the shadow-slot root store is emitted inline instead of calling into the runtime.**

Codegen roots every pointer-capable local by mirroring it into a shadow-stack
slot. Until now each such store was an `extern "C"` call —
`js_shadow_slot_bind(idx, &local)`, or `js_shadow_slot_set(idx, 0)` for the
"dead from here" clear. #7079 made those functions cheap *internally*, but a
call costs twice: the call itself, and the fact that it is **opaque to LLVM**,
which forces a spill of every live value around it and blocks hoisting across
it. That reading — that what remained was per-store calls only codegen could
touch — is what this change tests.

Read out of the shipped `aarch64` archive, `js_shadow_slot_bind`'s fast path
was ~35 instructions, of which only about six did any work:

| | before | after |
|---|---|---|
| call/return | `bl` + 4-instruction prologue/epilogue pair | — |
| reach the thread-local | TLSDESC `adrp`/`ldr`/`add` + **indirect `blr`** into the resolver, `mrs tpidr_el0` | — (state pointer already in a register) |
| lazy TLS destructor check | `ldrb`/`cmp`/`b.eq`, plus a `panic_access_error` edge | — (the TLS type is now drop-free) |
| the actual work | 2 guards, address computation, `stp`, gated barrier | unchanged |

**How the thread-local is reached.** Not by re-deriving the TLS address in
generated code — that would mean modelling Rust's TLS model per platform and
would be a second, unverified path to the same memory. Instead the address is
obtained *from the runtime*: `js_shadow_frame_enter` is `js_shadow_frame_push`
returning the address of this thread's `ShadowStackState` instead of the frame
handle, so codegen pays exactly the one thread-local lookup per activation that
the push already paid. The pointer is cached in an entry alloca; each store
loads it back and re-reads `ptr`/`len`/`frame_top` from it. Caching is sound
because it is the address of a `const`-initialized, drop-free `thread_local!` —
fixed for the thread's lifetime, never reallocated — while the *buffer* it
points at does move when a deeper frame grows it, which is why no frame base is
cached. The handle the matching `js_shadow_frame_pop` needs is recovered as
`frame_top - SHADOW_STACK_HEADER_SLOTS`, so the pop side is untouched.

**`ShadowStackState` is now `#[repr(C)]` with an explicit buffer.** Generated
code addresses its fields by hardcoded offset, and `Vec`'s layout is explicitly
not a stable contract — in the archive read at the time of writing it happened
to place `cap` at 0, `ptr` at 8 and `len` at 16, and a silent reorder would have
codegen writing live GC roots through the wrong word. Splitting the three words
out also drops the type's drop glue, which is what forced the per-op lazy
destructor-registration check; the buffer is now freed at thread exit by a
separate guard thread-local that is armed only from the cold growth path, and
that resets the state to the empty sentinel rather than leaving `std` to mark
the slot `DESTROYED` (whose next access aborted through `panic_access_error`).

**Soundness, per root property.**

*Liveness* — the inline store writes the same `ShadowEntry.value` and sets the
same `SLOT_ACTIVE` bit of `meta`, at the same index, so
`visit_shadow_stack_root_slots` marks it identically.

*Rewritability* — `meta` still carries the bound compiled-local address, with
`bound_slot_meta`'s alignment fallback intact (a tag-colliding address is
recorded active-but-unbound, never truncated), so an evacuating collection
rewrites the alloca the mutator reads after the safepoint, not just the mirror.

*The value the mutator stored* — the value is read from the local slot at the
store site, in the position the call occupied, and written immediately. Nothing
re-reads a slot at a later safepoint.

The incremental-mark root shading barrier is emitted inline behind the same
`PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` gate the runtime and
`emit_persistent_shadow_root_barrier` already use, so a pointer stored into a
root after the collector scanned roots is still shaded.

**Guards.** Both of the runtime function's guards are emitted: the
`frame_top == usize::MAX` sentinel test and the `slot < len` bounds check. The
sentinel is unreachable in a balanced program (that state implies `len == 0`,
so the bounds check alone would skip), but it is kept for exact parity with the
function it replaces, because the failure mode if the two ever diverge is silent
corruption rather than a skip: `usize::MAX + idx` wraps to `idx - 1`, an
*in-bounds* index into the frame header, unlinking every outer frame from the
root scan. That is the same wrap-around class #7079 fixed in `frame_pop`.

Each store also emits a null-state fallback arm that calls the original runtime
function. `js_shadow_frame_enter` is declared with a `nonnull` return, so LLVM
folds that arm away wherever the push dominates — verified in the linked
binary, where `js_shadow_slot_set` no longer appears at all.

**Not inlined, deliberately:** the frame push/pop pair (per activation, not per
store), the parameter and closure-prologue binds, and the persistent
entry-setup binds. Those are emitted before the lowering context exists and
would need the guard/barrier control flow restructured; they remain the largest
identified per-activation cost, and `alwaysinline` leaf functions turn them
into per-iteration cost, so they are the natural next step.

`PERRY_INLINE_SHADOW_SLOT=0`/`off`/`false` reverts to the calls for bisection.
It is part of both the build-level and per-object cache keys, so the two arms
can never share a cached `.o`.

Tests: 6 new runtime cases pinning the addressing contract (inline write
visible through `js_shadow_slot_get` and vice versa, liveness and rewriting
across a real copying minor, both guards, the clear preserving its binding), 5
new codegen cases pinning the emitted IR shape, and a `shadow_layout_contract`
test in `perry` — the only crate depending on both — asserting codegen's
offsets equal the runtime's, since nothing else would catch that drift and the
result would be silent. Every one was verified to fail under a targeted
sabotage of the thing it covers.
