### Fixed — the gap gate could never be green

`run_parity_tests.sh` can only record `node_fail` via `perry_abnormal_exit`, which
matches **signals and timeout only** (124, 132/134/136/138/139, >128). Node's real
failures in this suite are a plain `exit 1` — `ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX`
for the enum and parameter-property tests (strip-only mode cannot run them, and
Node 26 dropped `--experimental-transform-types`), and `MODULE_NOT_FOUND` for the
npm ones. Those fall through to a normal comparison and come out `parity_fail`.

**Ten of the fifteen `gap_snapshot.json` entries recorded `node_fail`, a status the
harness is structurally incapable of emitting.** `gap_snapshot.py diff()` therefore
returned `changed` for every one, and any diff makes it exit 1 — so the gate could
never pass, on any tree. It survived because `parity` is tag-gated and invisible
per-PR; `conformance-smoke` (which runs `run_gap_tests.sh --shard N/8`, the same
suite) is what surfaced it as 8/8 red shards in #8117.

This is the mirror of CLAUDE.md's "four ways a gate can be unable to **fail**": a
gate that cannot **pass** is equally uninformative, and rots the same way.

Resolved by category rather than by editing statuses — the schema says "do not
hand-edit", and hand-editing is how ten impossible statuses got there:

* **3 TypeScript-syntax tests** (`test_gap_4510_enum_forward_ref`,
  `test_gap_derived_param_props`, `test_gap_enum_in_function_body`) get
  `test-parity/expected/*.txt` fixtures — the channel the `test_decorators_*` tests
  already use for exactly this reason. Node can never run them, so the oracle was
  built with `esbuild` → node and cross-checked against the expectations written in
  each test's own header; Perry is byte-identical to all three. Note
  `test_gap_derived_param_props` already said *"this is a perry-only expected-output
  test"* — the fixture was simply never created, so it had verified nothing since May.
* **6 npm tests** (`backoff_options`, `cron_cronjob`, `dayjs_factory_arg`,
  `moment_methods`, `ratelimiter_memory`, `slugify_options`) had their packages
  declared as devDependencies. They were never in `package.json`, so the oracle could
  not run **in any environment, CI included** — `npm ci` would not install them
  either. `.npmrc` says the root npm install exists precisely to "materialize the
  parity-test fixture deps". All six now run under node with exit 0 and all six match
  Perry: live parity, which is stronger than a frozen fixture.
* **1 remaining, re-triaged as a real Perry bug** rather than a snapshot artifact.
  `test_gap_prop_plan_cache_invalidation` is written for sloppy mode but runs as ESM,
  which is strict, so its frozen-object writes throw. Node and Perry both correctly
  throw and differ only in the message: node names the constructor (`'#<H>'`), Perry
  hardcodes `'#<Object>'` at `error.rs:1726`, because
  `js_throw_type_error_immutable_write` takes `(kind, key_ptr, key_len)` and no
  receiver. Recorded with that root cause; the fix is a receiver-aware variant behind
  the existing `throw_immutable_write` wrapper (14 callers, no codegen ABI change).

The snapshot was then **regenerated from a full 562-test run** (`UPDATE_SNAPSHOT=1`),
not hand-edited: **99.1% parity, 5 non-passing entries, 0 crashes**, and
`gap_snapshot.py check` exits 0. `test_gap_iterator_helpers_2874`, which had been
listed while passing, is gone.

Also fixed: `scripts/gc_gate_wiring_check.py` did not list `check_gc_env_knobs.py`
in `GATES`, so the gate-integrity checker could not see one of the gates it exists to
audit. Now 8 gates, self-test green.

Two things deliberately not done. `SKIP_TESTS` in `run_parity_tests.sh` is exact-match
and holds only legacy `test_*` names, so it can never match a `test_gap_*` test — left
alone because every current case is better served by the expected-output channel.
And `test_gap_webcrypto_async_threadpool` failed once during an unsharded verification
run at load ~64 and was **not** recorded: it asserts that an async digest crosses a
macrotask boundary, node printed `false`, and 5/5 re-runs at load ~45 print `true`,
matching Perry. The oracle flaked, and parking it would have converted a load artifact
into a permanently accepted failure.

Refs #8117.
