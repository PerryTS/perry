- **fix(compile): stop leaving `perry_llvm_scratch_*` / `perry_native_reps_*.json` in `$TMPDIR` (#11495).** `--trace llvm` set `PERRY_LLVM_KEEP_IR=1` for the user, which is not part of its documented contract. `PERRY_SAVE_LL` already writes every module's `.ll` into `.perry-trace/llvm/`. KEEP_IR also left a `.perry-keep`-marked scratch dir, which the stale-scratch reaper is forbidden to remove, plus a native-reps JSON in the temp root for every module, and nothing ever deleted them. The gc-root-dominance corpus traces hundreds of compiles per run, and a shared build host reached 60k entries (18+ GB) in a day. `--trace llvm` no longer implies KEEP_IR. Set it explicitly to get those files, which still land in `$TMPDIR` as before.
  - A new RAII `TempPathGuard` (`perry-codegen/src/linker_temp.rs`) removes compile-scoped temp paths on every exit, including the early `?` returns that skipped the explicit cleanup. Those were:
    - a failed `.ll` write, e.g. ENOSPC, the very state a leak produces;
    - no assembler found in `finish_native_emission`;
    - a failed or missing `ld -r` in `merge_unit_objects`.
  - The guard leaves alone anything a retention decision claimed: `PERRY_LLVM_KEEP_IR`, or the one failed compile per process whose IR `FailedScratch` keeps for diagnosis.
  - Tests:
    - `crates/perry/tests/issue_11495_trace_llvm_temp_leak.rs` compiles plainly and with `--trace llvm` into a private `TMPDIR` and asserts it stays empty, while the trace dir did receive its `.ll`. Sabotage-checked: re-adding the `set_var` fails it with exactly `["perry_llvm_scratch_…", "perry_native_reps_….json"]`.
    - `temp_path_guard_removes_unless_retained` is sabotage-checked too: a no-op `Drop` fails it.

Keep both target-specific Inkwell generations in Cargo.lock after the main dependency refresh: Windows retains 0.9 while other targets use 0.10, so locked builds include both internal macro packages.
