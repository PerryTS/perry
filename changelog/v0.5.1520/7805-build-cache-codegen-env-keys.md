**The build cache keys every codegen env var, and a source scan now enforces it** (#7183).

#7161 turned `PERRY_GC_MOVING_LOOP_POLLS` into a compile-time gate — codegen emits or omits `js_gc_loop_safepoint` per `moving_safepoint_polls_enabled()` — without adding it to `build_cache.rs`'s env key list. Nothing bit, because the per-object cache keys it correctly, but the build-level no-op probe was one `-o` collision away from handing back a binary built under a different configuration, for the one arm that must not go dark: `=1` is the only configuration exercising the evacuating minor end to end.

The issue said "audit, don't just patch the one", so I did. Scanning `perry-codegen/src` for `env::var("PERRY_*")` found **35 of 47** reads absent from the list, not one — `PERRY_PTR_SHAPE_LOCALS`, `PERRY_SPECIALIZED_ABI`, `PERRY_INLINE_HOT_SMALL`, `PERRY_CODEGEN_UNITS`, the whole representation-selection family. All 30 that can change emitted code are now inputs. Adding a key only makes the probe more conservative, so the bias is inclusion.

Five are deliberately excluded, in a named `BUILD_CACHE_ENV_EXCLUSIONS` list rather than by omission: `PERRY_SAVE_LL`, `PERRY_LLVM_DIFF_DIR`, `PERRY_REPSEL_DEBUG`, `PERRY_STATEPOINT_REPORT` (side artifacts, same object bytes) and `PERRY_CODEGEN_UNIT_JOBS` (thread count; the partitioning it parallelises is keyed by `PERRY_CODEGEN_UNIT_SIZE`/`PERRY_CODEGEN_UNITS`, which are inputs).

**The list is now self-enforcing.** `codegen_env_vars_are_build_cache_inputs` scans `perry-codegen/src` and fails when a var is in neither list — because a hand-maintained list against a growing set of gates rots, and this one rotted silently once already. It fails in both directions: a stale exclusion naming a var codegen no longer reads is also an error, and it refuses to pass if the scan finds fewer than 20 vars, so a broken matcher cannot make it vacuously green.

It earned its keep immediately: it caught `PERRY_OPT_REPORT`, which my own audit had missed. That one is an exclusion — `opt_report`'s module doc states the contract, "Observational only. Nothing in this module is read by codegen … the returned fact sets are bit-identical with the report on and off, which the CLI's byte-identical-object test asserts."

Sabotage-verified rather than assumed: deleting `PERRY_GC_MOVING_LOOP_POLLS` from the list — reproducing #7161's exact omission — makes the test fail and name it; restoring it passes. `cargo test -p perry` is 903 passed / 0 failed.
