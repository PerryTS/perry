### perf(codegen): recursion-participating specialized clones use LLVM's `preserve_none` calling convention (#8175)

fib40 spent ~45% of wall time on a frame its ~165M leaf invocations never
used: a param-derived value live across a call was materialized into a
callee-saved register in the entry block, which pins the CSR save/restore
there and defeats LLVM shrink-wrapping. `preserve_nonecc` deletes the cause —
with no callee-saved registers there is nothing to pin, so the frame sinks
into the recursive path and the leaf runs frameless (5 instructions on
arm64, 3 on x86-64).

Mechanism: one module-level registry (`LlModule` → `RegCounter`) drives the
define header, the cross-unit declare line, and both call choke points
(`LlBlock::call`'s plain and invoke arms), so a call site can never disagree
with its callee's convention — a mismatch is UB, not a verifier error, and
`spec_preserve_none_tests::assert_preserve_none_consistency` scans rendered
modules for agreement in both directions. The in-process dialect reader
parses the token on define/call/invoke lines and sets the real LLVM
convention (`CallingConv::PreserveNone`) on both the function and each call
site, so text, native, and unit-split backends emit the same machine code.

Scope: only specialized clones that participate in direct recursion (Tarjan
SCC over `FuncRef` call edges, `collect_recursion_participants`) — the
boundary cost of entering a `preserve_none` callee from a normal-CC caller
(~20 CSRs saved once per entry) amortizes only under a recursive tree. Spec
clones are `internal` and direct-call-only by construction
(`spec_abi_symbol_reachability`), so the convention cannot escape a module.
Target-gated off watchOS `arm64_32` and ARM64 Windows, the same predicate
family as the RS4GC target-awareness. `PERRY_SPEC_PRESERVE_NONE=0` is the
single-binary A/B kill switch, keyed into the build and object caches.

Liveness gates: the recursive-fixture test counts convention-carrying call
sites (≥3: init site + both recursive edges), and an asm-level test runs the
exact in-process pipeline (RS4GC + `-O3` + target machine) and asserts the
clone's first instruction is not a frame store — a change that silently
stops applying the convention re-grows the pinned frame and goes red.

Measured on the quiet M1 mini (best-of-5, `/usr/bin/time -l`, both arms from
one compiler via the kill switch): per-row instruction + peak-RSS table in
PR #8203. 16 of 19 corpus rows compile byte-identically (the gate holds);
fib40/interp/iso_miss differ.
