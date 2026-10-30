# AGENTS.md

Perry compiles TypeScript to native executables: SWC parses, `perry-hir` lowers, `perry-transform` runs IR passes, `perry-codegen` emits LLVM, and the result links against `perry-runtime` and `perry-stdlib`.

This file holds only what the code cannot tell you and what is expensive to get wrong. Design and internals live in `docs/src/` (`docs/src/SUMMARY.md` is the shelf); point-in-time evidence lives in `docs/audits/`.

## Where the change goes

Find the crate that already owns the behavior before writing: the pipeline order is the map, so parsing sits in `perry-parser`/`perry-hir`, IR behavior in `perry-transform`, emitted code in `perry-codegen`, and anything that exists only at runtime in `perry-runtime`/`perry-stdlib`.

- `docs/src/contributing/architecture.md` carries the crate map and the module layout. `docs/src/contributing/crate-policy.md` carries the workspace categories, the dependency rules, and what justifies a crate at all; read it before moving code across a crate boundary or adding a member.
- `perry-runtime` owns values, objects, the heap and the GC; `perry-stdlib` owns the Node and Perry builtin implementations built on them. A builtin that needs the heap belongs in stdlib.

## Every change

- `main` is protected: PRs only, squash or rebase merge. The only required check is `pr-gate`; `scripts/ci_plan.py --table` shows what runs in each tier (`docs/src/testing/ci-tiers.md`).
- `.github/pull_request_template.md` is the contract a PR is reviewed against: one logical change, a summary, a test plan carrying the commands actually run, and the paths a user-visible change touches.
- Do not bump the version in a PR, maintainer-authored ones included. `[workspace.package].version` in `Cargo.toml` is the only copy, and the release maintainer bumps it when cutting a release.
- A change touching `crates/` adds `changelog.d/<PR>-<slug>.md` in the same PR, named for the real PR number. An all-zero `0000-` prefix fails `lint` outright; a wrong real number only warns, because backfills and stacked PRs legitimately name another PR. The `skip-changelog` label covers typo and CI-only churn. Never edit `CHANGELOG.md`; it is frozen.
- A change to the CLI, the stdlib or a runtime API updates `docs/src/` in the same PR; the `check` tier regenerates the API docs and asserts no drift.
- Commits keep the loose `feat:` / `fix:` / `docs:` / `chore:` prefix the log already uses.
- Read `MERGE_GUIDE.md` before landing a queue of PRs.

## Comments

A comment earns its lines only by saying what the code cannot: the invariant, the ordering constraint, the reason the plainer form is wrong. Perry is public and long-lived, so every line a reader pays for must be one a fresh reader needs, not one the author happened to learn.

- Write the present tense: the invariant as it holds now. A date, a `used to`, a `previously`, a `for now` describes the commit that wrote it, and the next change makes it false without touching it.
- A PR or issue number may accompany the reason, never replace it. `#6117` alone points into a closed conversation, and it keeps reading as current long after the code moved.
- Delete rather than paraphrase once a comment stops holding. The removed reasoning survives in `git log` and the pull request; the file holds the present.
- Rationale that must outlive a refactor belongs in `docs/src/`, which is reviewed prose. Code keeps the invariant and the pointer to it.
- The comments inside a hunk you edit are yours for that edit: fix or delete the ones that no longer hold instead of leaving them beside a change that contradicts them.

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
- `./scripts/test_affected_crates.sh --base origin/main` mirrors CI's scope: per-crate Cargo commands from committed, staged and unstaged paths (`--dry-run` prints them). Stage new files first, because untracked paths are ignored. It skips `crates/*/tests/`, and `cargo test --workspace` is not a substitute — per-crate feature sets stay isolated.
- `scripts/run_lint_gates.sh` runs every gate the CI `lint` job runs, plus the `warnings` and `check` arms the compile tier derives, in one command. Pick this over the gates that merely look topically relevant to your diff: a gate you did not run is indistinguishable from a gate that passed. `--list` prints the set; `BASE_SHA=origin/main` arms the merge-base comparisons, and `SKIP_COMPILE_GATES=1` says so in its summary while iterating. It needs PyYAML, which only `.github/workflows/test.yml` pins.
- Turnloop is the only event loop. No Tokio dependencies, including optional,
  transitive, target-specific, dev or build dependencies. Run
  `python3 scripts/tokio_inventory.py --offline` before changing dependencies.

## TypeScript Parity Status

The gap suite (`test-files/test_gap_*.ts`) is compared byte-for-byte against `node --experimental-strip-types`. The oracle is Node `26.12.1`, pinned in `.node-version`. Use that exact version locally: when Node cannot run a test, the harness marks it `node_fail` and drops it from the gate silently. The builtin-module matrix (`node scripts/node_compat_matrix.mjs --module <name>`) uses its own pin, `tools.node.version` — currently **26.12.1**, in `external-tools.json`.

A new regression test for one Node core module goes next to that module's existing cases in `test-parity/node-suite/` (for example `test-parity/node-suite/crypto/webcrypto/get-random-values.ts`); run it with `./run_parity_tests.sh --suite node-suite --module <module>`. Other self-contained Node-parity programs go in `test-files/test_parity_*.ts`. The full placement rules are in `test-parity/README.md`.

Before believing a red gap shard, A/B the named tests against a pristine `main` build.

## Silent failures

These compile and pass locally, then break somewhere else:

- **NaN-boxing:** use `f64::from_bits(u64)`, never `u64 as f64`. Do not box an already-boxed value. Tags are in `crates/perry-abi/src/lib.rs`.
- **Build cache:** a new `PERRY_*` env var read in a compiler-linked crate must go in `COMPILE_ENV_VARS` (or `COMPILE_ENV_EXCLUSIONS` with a reason) in `crates/perry/src/commands/compile/compile_env.rs`, or cached objects serve the old setting.
- **Threads:** a JSValue created on a worker thread is invalid on the main thread. Use `spawn_for_promise_deferred()` and convert on the main thread.
- **Handles:** dispatch has two registries, `HANDLE_METHOD_DISPATCH` and `HANDLE_PROPERTY_DISPATCH`. Register both.
- **GC roots in codegen:** a GC-managed value's root store must dominate every later site that can collect. Violations surface cycles later as `TypeError: value is not a function`. See `docs/src/internals/gc-rooting-invariant.md`.
- **GC roots in the runtime:** a cache holding a raw heap pointer is a root. Register it with `gc_register_mutable_root_scanner` (`crates/perry-runtime/src/gc/roots.rs`) in the same commit. A perfectly reproducible GC bug usually means an unrooted table.

Areas where the symptom is rarely the bug: the async-to-generator transform, native base-class subclassing, and the two prototype paths (`CLASS_PROTOTYPE_OBJECTS` vs `CLASS_DECL_PROTOTYPE_OBJECTS`). For GC bugs, `PERRY_GC_SCHEDULE_SEED=<u64> PERRY_GC_PROTECT_FROMSPACE=1` turns a stale pointer into an immediate fault; every knob is documented in `docs/src/internals/garbage-collector.md`.

## Verifying a runtime change

A clean `git status` says nothing about what you link.

- Rebuild with the `*-static` crates, pin `PERRY_RUNTIME_DIR`, and confirm the `.a` mtime moved after your edit. Otherwise both arms of an A/B link the same stale archive. Keep the package set identical across builds.
- A prebuilt binary in another worktree may come from that worktree's uncommitted changes. Build your own reference and check it first.
- Read the harness's exit code, not that of a wrapper pipe.

## Gates that ratchet

Most structural gates here compare the tree against a committed baseline instead of an absolute bound, so they fail on *growth*: `scripts/raw_handle_debt.py`, `scripts/runtime_size_budget.py`, `scripts/test262_ratchet.py`, and the committed baseline files they compare against.

- A baseline moves **down**. `--update` refuses to raise it, and CI also runs the differential arm (`--no-raise-vs <merge-base>`), so widening a baseline or a per-module ceiling to cover new debt fails the pull request from the other side. Remove the debt site instead.
- `scripts/check_file_size.sh` is the same idea plus an ALLOWLIST: a tracked `*.rs` over 2,000 lines — comment, test and blank lines priced the same as code — fails unless the script names it with a one-line rationale. An allowlisted file has no size protection left, so hold it to the cap anyway, and treat adding an entry as the last resort that names the follow-up. Its error text carries the split recipe: extract a function group into a sibling module and re-export it from `mod.rs` with explicit named `use` statements, since globs do not propagate through transitive re-exports.
- Splitting a module that carries ledgered debt requires the ledger line to travel with it: the moving line takes a trailing `# moved-from: <old path>` so the merge-base arm credits what the source gave up (`scripts/raw_handle_debt.py`, "RELOCATIONS"). Without it, a module whose debt is spread across it cannot be split at all.
- A new test file in a suite with an explicit registry is no test at all until it is registered. `scripts/check_test_registration.py` runs in `lint`; `docs/src/testing/test-registration.md` says which suites glob and which keep a list.

## Four ways a gate can be unable to fail

Check all four when adding or reviewing a gate:

1. **`continue-on-error: true`** on the job.
2. **Not in branch protection's required contexts**: it reports failure without blocking.
3. **Unconditional `cancel-in-progress`**: each merge cancels the previous `main` run. Scope cancellation to `pull_request`.
4. **The gate runs but its subject never did**: assert the subject was live (for example `copied_objects > 0`), not merely that nothing threw.

A new gate has never been green: run it once, then make it required.

## GC knob kill-policy (binding)

Every GC env knob has a required CI arm exercising its OFF state, or it is deleted after one release of soak. At most one diagnostic-only knob may exist at a time, labelled untested. When a knob's off-state stops being exercised, delete the off-state and the code behind it.
