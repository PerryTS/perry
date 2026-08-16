`cargo-test` is green again on Linux. Four `perry-codegen` tests
(`rs4gc_canonicalizes_construction_time_folds_before_root_liveness` and the
three `native_emit` construction-path tests) assert that textual and native
construction emit byte-identical output. The generated code had already
converged; the objects differed only in the name LLVM records for the module.

Nothing set `source_filename`, so LLVM fell back to whatever path reached the
assembler: the textual pipeline writes each module to a per-call temp file, so
its recorded name carried a random nonce (`perry_llvm_<nonce>.ll`), while
native construction recorded its in-memory module id (`perry_native_module`).
ELF stores that name as an `STT_FILE` symbol, so the two paths could never
agree and neither was reproducible run to run. Mach-O records no such symbol,
which is why all four passed on a macOS host and failed only on the Linux
runner.

All three module-header sites (`to_ir`, `skeleton_ir`, and the per-codegen-unit
prologue) now emit an explicit `source_filename`, so the recorded name is the
same constant on both paths and independent of the temp path. Emitted objects
no longer embed a random temp filename, so they are reproducible across runs.

The three `native_emit` tests only ever ran against the host triple, so on a
macOS machine they exercised Mach-O exclusively — the one object format that
does not record this name. `native_and_text_arms_agree_on_an_elf_target` now
pins the comparison to `x86_64-unknown-linux-gnu` and asserts the bytes start
with `\x7fELF` before comparing, so it cannot pass by testing the wrong format;
it is sabotage-tested against the un-fixed emission.
