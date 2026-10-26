# AGENTS.md

Perry compiles TypeScript to native executables: SWC parses, `perry-hir` lowers, `perry-transform` runs IR passes, `perry-codegen` emits LLVM, and the result links against `perry-runtime` and `perry-stdlib`.

This file holds only what the code cannot tell you and what is expensive to get wrong. Design and internals live in `docs/src/`; point-in-time evidence lives in `docs/audits/`.

## Every change

- `main` is protected: PRs only, squash or rebase merge. The only required check is `pr-gate`; `scripts/ci_plan.py --table` shows what runs in each tier (`docs/src/testing/ci-tiers.md`).
- Do not bump the version in a PR, maintainer-authored ones included. `[workspace.package].version` in `Cargo.toml` is the only copy, and the release maintainer bumps it when cutting a release.
- Add `changelog.d/<PR>-<slug>.md` (see `changelog.d/README.md`). Never edit `CHANGELOG.md`; it is frozen.
- Read `MERGE_GUIDE.md` before landing a queue of PRs.

## Building and testing

```bash
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
RUST_TEST_THREADS=1 cargo test --release -p perry-runtime
./scripts/run_gap_tests.sh
cargo run --release -- file.ts --trace hir --focus fnName   # one function's HIR
cargo run --release -- file.ts --trace llvm                 # IR in .perry-trace/llvm/
```

- `perry-runtime` and `perry-stdlib` are rlib-only. The `.a` archives that `perry compile` links come only from the `*-static` wrapper crates.
- `perry-runtime` tests share process-global state. Run them single-threaded; a fixture-precondition failure under parallel threads is interference, not a regression.
- `crates/*/tests/*.rs` suites run on a PR only when the diff names them. Put acceptance coverage in unit tests.
- Files are capped at 2000 lines (`scripts/check_file_size.sh`).

## TypeScript Parity Status

The gap suite (`test-files/test_gap_*.ts`) is compared byte-for-byte against `node --experimental-strip-types`. The oracle is Node `26.12.1`, pinned in `.node-version`. Use that exact version locally: when Node cannot run a test, the harness marks it `node_fail` and drops it from the gate silently. The builtin-module matrix (`node scripts/node_compat_matrix.mjs --module <name>`) uses its own pin, `tools.node.version` — currently **26.12.1**, in `external-tools.json`.

A new regression test for one Node core module goes next to that module's existing cases in `test-parity/node-suite/` (for example `test-parity/node-suite/crypto/webcrypto/get-random-values.ts`); run it with `./run_parity_tests.sh --suite node-suite --module <module>`. Other self-contained Node-parity programs go in `test-files/test_parity_*.ts`. The full placement rules are in `test-parity/README.md`.

Before believing a red gap shard, A/B the named tests against a pristine `main` build.

## Silent failures

These compile and pass locally, then break somewhere else:

- **NaN-boxing:** use `f64::from_bits(u64)`, never `u64 as f64`. Do not box an already-boxed value. Tags are in `crates/perry-abi/src/lib.rs`.
- **Build cache:** a new `PERRY_*` env var read in a compiler-linked crate must go in `COMPILE_ENV_VARS` (or `COMPILE_ENV_EXCLUSIONS` with a reason) in `crates/perry/src/commands/compile/compile_env.rs`, or cached objects serve the old setting.
- **Threads:** a JSValue created on a tokio worker is invalid on the main thread. Use `spawn_for_promise_deferred()` and convert on the main thread.
- **Handles:** dispatch has two registries, `HANDLE_METHOD_DISPATCH` and `HANDLE_PROPERTY_DISPATCH`. Register both.
- **GC roots in codegen:** a GC-managed value's root store must dominate every later site that can collect. Violations surface cycles later as `TypeError: value is not a function`. See `docs/src/internals/gc-rooting-invariant.md`.
- **GC roots in the runtime:** a cache holding a raw heap pointer is a root. Register it with `gc_register_mutable_root_scanner` (`crates/perry-runtime/src/gc/roots.rs`) in the same commit. A perfectly reproducible GC bug usually means an unrooted table.

Areas where the symptom is rarely the bug: the async-to-generator transform, native base-class subclassing, and the two prototype paths (`CLASS_PROTOTYPE_OBJECTS` vs `CLASS_DECL_PROTOTYPE_OBJECTS`). For GC bugs, `PERRY_GC_SCHEDULE_SEED=<u64> PERRY_GC_PROTECT_FROMSPACE=1` turns a stale pointer into an immediate fault; every knob is documented in `docs/src/internals/garbage-collector.md`.

## Verifying a runtime change

A clean `git status` says nothing about what you link.

- Rebuild with the `*-static` crates, pin `PERRY_RUNTIME_DIR`, and confirm the `.a` mtime moved after your edit. Otherwise both arms of an A/B link the same stale archive. Keep the package set identical across builds.
- A prebuilt binary in another worktree may come from that worktree's uncommitted changes. Build your own reference and check it first.
- Read the harness's exit code, not that of a wrapper pipe.

## Four ways a gate can be unable to fail

Check all four when adding or reviewing a gate:

1. **`continue-on-error: true`** on the job.
2. **Not in branch protection's required contexts**: it reports failure without blocking.
3. **Unconditional `cancel-in-progress`**: each merge cancels the previous `main` run. Scope cancellation to `pull_request`.
4. **The gate runs but its subject never did**: assert the subject was live (for example `copied_objects > 0`), not merely that nothing threw.

A new gate has never been green: run it once, then make it required.

## GC knob kill-policy (binding)

Every GC env knob has a required CI arm exercising its OFF state, or it is deleted after one release of soak. At most one diagnostic-only knob may exist at a time, labelled untested. When a knob's off-state stops being exercised, delete the off-state and the code behind it.
