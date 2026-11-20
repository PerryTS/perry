### In-process LLVM native backend: build today's RS4GC IR, unfreeze the corpora (#7982)

`PERRY_LLVM_INPROCESS=native` could not build any function containing an
RS4GC root slot — effectively every function — so every `main` execution of
the `llvm-inprocess` native-backend job failed at
`%r5 = alloca ptr addrspace(1)`.

It was not a one-liner. Each fix exposed the next shape: `basic_type` had no
`addrspace(N)` arm; `ty_and_val` split `ptr addrspace(1) null` into type
`ptr` and value `addrspace(1) null`, because the qualifier belongs to the
type but sits after a space; `constant` produced an addrspace(0) `null`
regardless of the operand type, which the verifier rejects when stored into
an `alloca ptr addrspace(1)`; define lines carry `"frame-pointer"="non-leaf"`
and `gc "statepoint-example"` (the latter contains a space, so the
whitespace-split attribute loop saw two junk tokens); callsites carry
`"gc-leaf-function"`; and landing pads are now `landingpad token cleanup`,
whose `token` result is not an inkwell `BasicType` and so is built through
llvm-sys. The `{ptr, i32} catch` pad still occurs and keeps its branch.

**Two defects were hiding behind the reader failure, both worse than it.**

*The native path returned assembly and called it an object.* The statepoint
backends compact the stack map at assembly time, so the plan carries `-S`.
The textual in-process path has always run the rewrite-and-assemble step
afterwards; the native and diff paths returned the bytes straight to the
object cache and the link died with `ld: unknown file type` — which the
textual path's own comment predicts, for the path that already handled it.
Now factored into `linker::finish_native_emission` and wired into all four
native/diff emit sites.

*A natively-constructed module had NO GC strategy, so RS4GC never ran on it.*
`native_emit::synth_define_header` was a second, independent copy of
`LlFunction::to_ir`'s header renderer, written before
`"frame-pointer"="non-leaf"` and `gc "statepoint-example"` existed and never
updated. The result verifies, links and executes correctly on any program
that does not collect, while having **no precise roots at all** — #7332's
shape, and invisible to a behaviour-parity smoke arm by construction, since
identical output on a non-collecting program is exactly what it produces.
Fixed structurally rather than by testing for agreement: both callers share
`LlFunction::define_header`, and the pin lives in `function.rs`, which
compiles without the `llvm-inprocess` feature and therefore runs in per-PR
CI.

**The corpora — the half arguably worth more than the fix.** All three
tracked `.ll` files froze on 2026-08-03, 151 codegen commits earlier, with
zero `addrspace(1)`. `corpus_spike ... ok` proved the tests RAN, not that
they test today's IR: CLAUDE.md's fourth way a gate cannot fail, occurring
inside the liveness assert written to prevent it. The same thing had happened
nine days before (#7310, stale setjmp calls), which is the argument against
refreshing by hand a third time.

* `scripts/refresh_llvm_inprocess_corpora.sh` regenerates all three from the
  built compiler (`--check` diffs instead of writing).
* `scripts/check_llvm_corpus_currency.py`, added to `lint`, asserts every IR
  form the reader carries a dedicated branch for is PRESENT in the corpora, so
  a form that disappears must be either refreshed back in or have its reader
  branch deleted. Sabotage-verified against the corpora this change replaces:
  pointed at them it names all 10 forms they lacked. Stated limit: it catches
  a form that vanishes, not one codegen newly invents — nothing static can, and
  the end-to-end `native` arm is the closure for that direction.
* `llvm-inprocess.yml`'s existing age diagnostic was itself vacuous: `git log`
  on the default depth-1 checkout printed a confident `0` commits behind
  however stale the files were. Now `fetch-depth: 0`, and it fails loudly
  rather than printing zero if history goes missing again.

Corpora refreshed to 497 / 1082 / 420 `addrspace(1)` sites; the EH corpus
keeps 84 invoke edges and its personality clause. `dialect/mod.rs` crossed
the 2000-line cap, so type and constant parsing moved to `dialect/types.rs`.

Validated against LLVM 22.1.4: 934 `perry-codegen` lib tests green including
all three corpus round-trips, and `PERRY_LLVM_INPROCESS=native` compiles and
runs both the spike and the try/catch program with output byte-identical to
the textual backend.

Not closed here, and separate defects — neither was ever reached in CI
because the native arm failed first: `=diff` still reports a byte mismatch on
the spike (149,105 text vs 163,902 native; it was 50,995 before the
GC-strategy fix, so the gap narrowed from "RS4GC never ran" to a real but far
smaller divergence), and the unit-split diff arm fails with `call to
undeclared @js_shadow_slot_set`.
