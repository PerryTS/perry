**`fix(test-isolation)`: two process-global sinks recorded across test boundaries.**

Both follow one pattern: a **process-global collector** paired with a lock that only serialises the tests which *take* it. Any concurrently-running test that exercises the recording path — without knowing the collector exists — wrote into the lock-holder's snapshot.

- **`opt_report`** — `FORCED` and the `Mutex<Vec<Entry>>` sink are global; `Session` drains on entry and drop but cannot stop a neighbour emitting mid-test. `only_the_return_position_is_marked_served` asserts `rows.len() == 2` and failed at **3**.
- **`ext_registry`** — `USED_PROVIDERS` is a process-wide `Mutex<HashSet>`; `record_ffi_call` fires from any lowering of an ext symbol. `ext_prefix_net_does_not_over_match` asserts the set is empty after an unlisted symbol and failed with `ioredis` still in it.

Both are **pre-existing** and both were surfaced by #7662 (Layer 1 slice 7), which added `child_process` and Proxy/Reflect lowering tests and so changed the parallel schedule: `only_the_return_position_is_marked_served` was 0/14 on `main` and 2/18 on that branch; `ext_prefix_net_does_not_over_match` was 1/20.

**Diagnosed from the extra row, not from the timing.** A 1-in-6 flake invites a retry; the value 3-instead-of-2 says a neighbour's entry is in the snapshot, which names the mechanism directly.

Recording is now narrowed to the thread holding the guard — `test_support::recording_thread_is_current()` for `opt_report`, `ProviderTestGuard` + `provider_recording_permitted()` for `ext_registry`. **Production is untouched in both**: the `opt_report` check sits inside the `#[cfg(test)]` forced branch and the env-var path is unchanged, and `PROVIDER_TEST_THREAD` is `#[cfg(test)]` with the predicate returning `true` whenever no provider test is running.

Verified 0 failures in 25 consecutive `cargo test -p perry-codegen --lib --no-fail-fast` runs on top of #7662, where the pair reproduced at 3/38 before.
