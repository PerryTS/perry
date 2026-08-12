Document that `perry-runtime`'s tests must run single-threaded locally, matching what every CI path already does.

**Why.** `gc::tests::root_words::bare_address_in_shadow_slot_survives_a_real_collection` was reported failing on a clean `c2a96b638` with the box at load ~111, passing in isolation and passing three full-suite reruns on a lightly loaded box. It was investigated as a possible load-dependent GC bug. It is not one.

**What it actually is.** `perry-runtime`'s tests share process-global side tables. `test.yml` has pinned `RUST_TEST_THREADS=1` for this crate since #1444 — the comment there says the default pool "races the GC/threading tests into intermittent SIGSEGV" — and `gc::tests::global_sink_isolation`'s header records that ~180 readers are still not required to take the clearing lock (#7672 converts them to `per_test_global!` one table at a time). The command CLAUDE.md documented for local runs, `cargo test --release --workspace`, does not pin the thread count, so it runs this crate in exactly the configuration CI exists to avoid. That divergence is the bug being fixed here.

**Evidence.** Against `2e5bf4434` (release, includes #7317 so the seeded-schedule machinery is present):

- 4 × 150 full-suite runs at `--test-threads=16`, sustained load ~90: **600/600 clean**, 0 vacuous.
- 4 × 80 full-suite runs at `--test-threads=64`, load ~115: **2 failures in 320** — `proxy::tests::object_array_numeric_write_guard_requires_complete_uniform_proof` and `array::element_shape::matrix_tests::matrix_delete_revokes`. Neither is `root_words`; the failing test is simply whichever one loses the race.
- Both failures are the crate's own vacuity guards firing ("fixture must start proven, or every verdict below is vacuous"; "one-field loops should publish one non-zero 16-bit lane") — a test finding its *precondition* destroyed by a concurrent thread, not a GC invariant violated. That distinction is what rules out the GC-bug reading.
- `root_words` itself did not fail once in 920 parallel full-suite runs, 1500 targeted `root_words`+`global_sink_isolation` pairings, or a 40-seed `PERRY_GC_SCHEDULE_SEED` sweep (rate 25%) — it never appears in that sweep's failures.

So the reported symptom is real and load-dependent, but it belongs to the test harness, not the collector. No test is `#[ignore]`d and no retry is added: the fix is to stop documenting the unsupported configuration.

**Note for the seeded-schedule sweep.** `PERRY_GC_SCHEDULE_SEED` is a whole-process env var, and `scripts/gc_schedule_fuzz.sh` takes a *compiled binary*, not the unit-test suite. Setting it across `cargo test` fails 8 GC tests at every seed (several assert the unset behaviour outright, e.g. `schedule::unset_is_inert_for_evacuation_policy`). That is the tool being used outside its contract, not a defect — recorded here so the next person does not re-derive it.
