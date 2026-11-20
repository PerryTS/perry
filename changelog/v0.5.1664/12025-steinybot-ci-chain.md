Landed @steinybot's CI-fix chain (#11979–#12005) against current main (thanks @steinybot):

- Removed dead code and an unused `mut` that failed the `-D warnings` job.
- `PERRY_REGION_ELEMENTS`, `PERRY_REGION_JOINT`, `PERRY_NUMBER_LOCAL_LOOP` and `PERRY_REGION_VIEWS` are now build-cache inputs.
- `crypto.hash(...)` through a namespace value roots its hash object across `update`.
- Short (inline) string keys are read through the SSO-aware accessors in private field reads, class field definition, class templates, worker error clones and zlib `off`.
- turnloop_client roots and rewrites a bound `AbortSignal`'s cancellation key, so `controller.abort()` still cancels a fetch after a copying minor moves the signal.
- A retiring worker agent releases its zlib streams, listeners and queued events. The `PERRY_GC_PROTECT_OLD_SWEEP` quarantine forgets spans in blocks that thread exit frees.
- CI gate repairs: split four files over the 2000-line cap; fixed `#[path]` resolution in `check_thread_locals.py`; budgeted-sweep keys in `gc_matrix_fixture_env.py`; regenerated the native-handle ledger, wasm32 runtime ABI table, string-payload baseline and shape-descriptor census; added thread-exit verdicts; registered `test_gap_gc_11862`; added the missing Windows gc_effects rows; added barrier-stem IR witnesses.
