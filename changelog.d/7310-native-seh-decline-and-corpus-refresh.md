### In-process LLVM backend: decline SEH funclets, refresh the stale corpora (#7302 / #7301)

Three follow-ups to the EH migration, all found by auditing what the native
backend actually covers.

**1. `--target windows` + try/catch was a hard compile error.** Perry lowers
exceptions to SEH funclets on windows-msvc (`catchswitch`/`catchpad`/
`catchret`), which the in-process reader cannot construct — inkwell 0.9
exposes no `build_catch_switch`/`build_catch_pad`/`build_catch_ret` (only an
opcode enum for *reading* them), so real support needs raw `llvm-sys` FFI.
Reproduced: `PERRY_LLVM_INPROCESS=native … --target windows` on any
try/catch (or async) program failed with `unknown instruction catchswitch`.
Such modules now decline to the textual path — costing only the in-process
speedup — instead of failing the build. The decline is narrow (windows
triple AND a personality present), so the macOS EH path added in #7307 is
untouched, and it is *not* the blanket personality bail #7307 removed.

**2. The tracked corpora were stale.** `spike_text.ll` and `batch_kernel.ll`
were captured before #7305 and still contained `setjmp` calls and the
`#0`/`#1` attribute groups — so the reader's primary gate was validating IR
Perry no longer emits, while missing the forms it does. Regenerated from
current codegen: the spike corpus now carries 38 `invoke` edges and zero
setjmp, and both round-trip through native construction and the LLVM
verifier.

**3. The setjmp-era attribute handling is deleted.** With the corpora
refreshed, nothing emits or contains `#0` (returns_twice) or `#1` (noinline),
so the reader's branches for them are gone — the losing mode stops compiling
rather than lingering as an untested branch (CLAUDE.md kill policy). A corpus
that still carries one now fails loudly as stale input, which is the point.

Verified locally against LLVM 22.1.4: liveness, behavior parity and
**byte-identical object verdicts** on the spike (21,393 bytes), the 3-unit
batch kernel (56,296 bytes) and the try/catch program (46,745 bytes);
569 `perry-codegen` tests green.
