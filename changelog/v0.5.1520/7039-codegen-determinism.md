Fixed non-deterministic LLVM IR: `emit_artifacts` iterated `hir.closure_source_text`
(a `HashMap`) when emitting retained closure-source constants, so the `@.str.N`
numbering of those constants was a per-process permutation — the same input
compiled to different IR on every run.

This was not cosmetic. It silently invalidates any A/B comparison of raw LLVM IR,
a technique several representation-selection and GC investigations relied on for
evidence. PR #7037 hit it directly: its first proof that `--opt-report` was
observational passed on nondeterministic output and had to be redone with four
runs per arm and a normalizer.

Measured over 6 compiles of one file (md5 of the concatenated per-module `.ll`):
5 distinct hashes before, 1 after. Emission order is the only thing that changes.
