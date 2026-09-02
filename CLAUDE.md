# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live as `changelog.d/` fragments (folded into GitHub Release notes at each tag); `CHANGELOG.md` is a frozen archive (≤ v0.5.1264).

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.1519


## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`). Compared byte-for-byte against `node --experimental-strip-types`. Run via `./scripts/run_gap_tests.sh` (a thin wrapper over `run_parity_tests.sh --filter test_gap_` that builds the compiler itself and gates on no new untriaged failures).

**The oracle is Node `26.5.1`, pinned in `.node-version` at the repo root** (the FILE is authoritative — this prose drifted once, #7599) — the single source of truth every CI workflow reads via `setup-node`'s `node-version-file`. **Run the gap suite against that exact version locally**, or your results won't match CI. The version is a *correctness input*, not an incidental toolchain detail: when node can't run a test (a feature newer than the pinned node), node exits non-zero, the harness classifies it `node_fail`, and the test is **silently dropped from the gate** rather than going red. CI sat on Node 22 while the suite grew Node 24/26 features, which hid 14 tests — all of Temporal, plus DisposableStack, Float16Array, and `Uint8Array` base64/hex (#6364). Node patch releases also change observable output (error-message text, `v8` heap fields), which is why the pin is exact. Raising it is a deliberate act: measure the failure delta under both oracles first, then triage what it exposes.

**Don't audit this by reading prose — run `python3 scripts/check_node_version_consistency.py --list`** (it is a `lint` step, so it is a required check). It re-derives every restatement of a Node version from the file that restatement quotes, and requires every literal `node-version:` in a workflow to be a *registered* exemption carrying a reason. That registry is the authoritative exemption list; the three below are what it currently holds, and an entry that stops matching the tree fails, so a bump must update or delete its own exemption.

Three workflows are deliberately exempt and say so inline: `node-core-subset.yml` derives its Node from `test-compat/node-core/pinned-version.txt` (it runs Node's *own* test corpus, which must match its own Node line), and the two release workflows (`release-packages.yml`, `release-hono-server.yml`, both Node 26) use Node purely as an npm *publishing* toolchain — pinned to a bare major on purpose, so a gap-suite oracle bump can never move the runtime that publishes releases. A fourth file, `benchmarks/public-baseline-config.json`, still pins **Node v22.23.1** for the *published* performance comparison: it is in `public_baseline.HARNESS_PATHS`, so editing it invalidates the committed artifact and reddens `lint` until the baseline is regenerated (~2 h on the quiet mini). The pin and the measurement are atomic by design — see the exemption's own text for the runbook.

**Last full sweep:** run `./run_parity_tests.sh` for the current snapshot. The umbrella tracker is #793 (Node.js + TypeScript compatibility roadmap); the previously-cited #447–#452 batch closed on 2026-05-04. Currently-open trackers worth knowing about:

- **Effect framework end-to-end (#321)** — `#684` (Schema.ts ~310th-init `(number).slice` regression) and `#809` (object-literal computed-keys + cross-module spread) are the live HashRing/Schema blockers.
- **Async context** — `AsyncLocalStorage` (real tracking across `await`/microtasks/timers, `#788`) and `async_hooks.createHook` (real lifecycle + asyncId, `#789`) both landed (closed 2026-05-16); these are no longer stubs.
- **Compile-as-package** — `#348` (ink TUI end-to-end), `#488/#489` (Drizzle + MySQL), `#678` (linker emits native callsites for V8-fallback modules).
- **Test/CI mechanics** — `#794` (per-category parity thresholds), `#796` (gap-suite output truncation + O(n²) `normalize_output`), `#812` (42-module behavioral matrix), `#806/#807/#808` (test harnesses for mixins / async context / ≥300-init scale).
- **Skip-list audit** — `#797` covers `test-parity/known_failures.json` provenance (issue # + date per entry).

### Node builtin compatibility matrix (`scripts/node_compat_matrix.mjs`)

Breadth sweep over EVERY `require("module").builtinModules` entry, both import forms (`M` and `node:M`), against a **pinned, SRI-verified Node** (the "latest stable" oracle, pinned in `external-tools.json` `tools.node.version` — currently **26.5.1**, independent of the `.node-version` gap-suite oracle). It compares Perry's export-SHAPE fingerprint (sorted `name:typeof` over the module namespace + the default export's typeof) to the oracle's. This is the systematic version of the #812 "42-module behavioral matrix" — shape, not deep behavior (behavioral cases stay in the node-suite).

```bash
# FAST LOOP — reach for this first when iterating on ONE builtin:
node scripts/node_compat_matrix.mjs --module fs                      # one module, both forms
node scripts/node_compat_matrix.mjs --module fs,path,crypto          # a few
node scripts/node_compat_matrix.mjs --module fs --method readFileSync,promises  # only these exports
node scripts/node_compat_matrix.mjs --only fs.readFileSync,path.join # combined mod.export form
# (the pinned Node download is skipped once cached under .cache/node-pin/)

# FULL SWEEP + GATE:
node scripts/node_compat_matrix.mjs                 # whole matrix + summary table
node scripts/node_compat_matrix.mjs --check         # CI gate: exit 1 on regressions vs the baseline
node scripts/node_compat_matrix.mjs --update-baseline   # rewrite test-parity/node-compat-matrix.baseline.json
```

A `--module` selector scopes `--check`/`--update-baseline` to just that slice (a single-module refresh never rewrites the whole baseline). A `--method`/`--only` subset is a print-only fast diagnostic (it narrows the fingerprint, so it is refused for `--check`/`--update-baseline`). **Bump the pinned Node** by editing `tools.node.version` in `external-tools.json` (add per-platform sha512 SRI), then `--update-baseline` and review the diff. Needs the release binary (`cargo build --release -p perry`). Full page: `docs/src/testing/node-compat-matrix.md`.

**Known categorical gaps**: `console.dir`/`console.group*` formatting, lone surrogate handling (WTF-8). (Lookbehind regex is NOT a gap anymore: `perry-runtime/src/regex.rs` falls back from the `regex` crate to `fancy-regex` for lookbehind/backreferences, with capture-group translation and replacement expansion.)

## Workflow Requirements

**Default flow is PR-based.** `main` is protected: pushes require a pull request, CI must pass, and only squash or rebase merges are allowed (no merge commits, linear history enforced). **The single required status context is `pr-gate`** — the fan-in of `test.yml`'s PR tier (`lint`, `check`, `warnings`, scoped `cargo-test`, the 6-shard fast-mode gap suite, `gc-stress`, `e2e-scoped`, and `security-audit` when a lockfile/manifest changed). What runs in which tier is decided by `scripts/ci_plan.py` (`--table`), documented in `docs/src/testing/ci-tiers.md`: **pr** (every PR push, ~11 jobs, must be green on `main`), **sweep** (every push to `main`, coalesced), **full** (nightly / tags / dispatch / `run-extended-tests` label — parity, compile-smoke, doc-tests, package smokes, the auto-optimize gap shards). Releases wait for a `full-suite-gate` on the release SHA. The satellite GC/perf gates run on PRs only with the `run-extended-tests` label; their six-hourly `main` sweeps are unchanged. Admins can bypass for hotfixes/version bumps, but the standard path is:

1. Branch from `main`, push, open a PR.
2. Wait for required checks to go green.
3. Squash- or rebase-merge. The PR branch auto-deletes on merge.

**For every change that lands on `main`** (whether via PR or admin bypass):

1. **Bump version**: Increment patch in `[workspace.package].version` in `Cargo.toml` and the `**Current Version:**` line above. That is the ONLY metadata edit CLAUDE.md needs.
2. **Add a changeset**: create `changelog.d/<PR>-<slug>.md` with the entry body (no version header — see `changelog.d/README.md`). Long-form root-cause writeups, file paths, validation notes all belong in the fragment, NOT in CLAUDE.md. **Never append to `CHANGELOG.md`** — it is frozen at v0.5.1264. Fragments are folded into the GitHub Release notes at tag time (`scripts/cut_release_notes.sh`) and deleted.
3. **Commit changes**: Include code, `Cargo.toml`/`Cargo.lock`, `CLAUDE.md` (version bump only), and the `changelog.d/` fragment together.

**Do not write changelog entries into CLAUDE.md.** This file is for orientation (architecture, common pitfalls, build commands). Per-change history lives in `changelog.d/` → GitHub Releases so CLAUDE.md stays small and stable across context loads.

### External contributor PRs

PRs from outside contributors should **not** touch `[workspace.package] version` in `Cargo.toml` or the `**Current Version:**` line in `CLAUDE.md`. The maintainer bumps the version at merge time — usually by rebasing the PR branch and amending. This avoids the patch-version collisions that happen when Perry's `main` ships several commits while a PR is in review (each on-main commit bumps the version; a PR that bumped to the same patch on day 1 is already behind by merge day). Contributors **do** write their own `changelog.d/<PR>-<slug>.md` fragment — the filename is PR-keyed, so in-flight PRs never collide.

## Build Commands

### Which profile to use

- **Local dev / testing (default choice)**: `cargo check -p perry` for fastest feedback, then `cargo build --profile perry-dev -p perry` (opt-level=1, codegen-units=16, incremental, no LTO — minutes instead of ~30). Use this for iterating on the compiler, running gap/parity tests, and reproducing bugs. Only fall back to `--release` if a bug is optimization-sensitive.
- **Shipping / official artifacts**: `--profile dist` (mirrors `release`: thin LTO, codegen-units=1, opt-level=3, strip). Slow by design — LLVM codegen runs single-threaded per crate at codegen-units=1, and the giant crates (perry-runtime ~340k lines, perry-codegen, perry-hir) serialize the build regardless of core count. Don't use it for iteration.
- **Local release-ish build when you need release perf**: override the compile-time killer, keep the optimization: `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release` (2–4× faster, ~1–3% runtime cost).

```bash
cargo build --release                          # Build all crates
cargo build --profile perry-dev -p perry       # Fast local dev build (#5422; perry-dev profile)
cargo build --release -p perry-runtime -p perry-stdlib  # Rebuild runtime (MUST rebuild stdlib too!)
cargo build --release -p perry-runtime-static -p perry-stdlib-static  # Emit libperry_{runtime,stdlib}.a (#5422: runtime/stdlib are now rlib-only; the .a comes from these wrapper crates)
RUST_TEST_THREADS=1 cargo test --release -p perry-runtime   # MUST be single-threaded (see below)
cargo test --release --workspace --exclude perry-runtime \
  --exclude perry-ui-ios --exclude perry-ui-tvos --exclude perry-ui-watchos \
  --exclude perry-ui-visionos --exclude perry-ui-android --exclude perry-ui-windows \
  --exclude perry-ui-gtk4   # Run tests (exclude cross-host UI crates on macOS)
cargo run --release -- file.ts -o output && ./output    # Compile and run TypeScript
cargo run --release -- file.ts --print-hir              # Debug: print HIR
cargo run --release -- file.ts --trace hir --focus fnName  # Debug: focused HIR for one fn (use to localize a miscompile)
cargo run --release -- file.ts --trace llvm             # Debug: dump per-module LLVM IR to .perry-trace/llvm/
```

When debugging a "compiled to the wrong thing" bug, reach for `--trace hir --focus <name>` to dump just the offending function's lowered HIR (functions/methods/classes matching the substring; import/init noise suppressed) instead of scrolling a full `--print-hir`. `--trace llvm` writes per-module `.ll` (it forces a no-cache rebuild so codegen actually runs). See `docs/src/cli/flags.md`.

## Architecture

```
TypeScript (.ts) → Parse (SWC) → AST → Lower → HIR → Transform → Codegen (LLVM) → .o → Link (cc) → Executable
```

| Crate | Purpose |
|-------|---------|
| **perry** | CLI driver (parallel module codegen via rayon) |
| **perry-parser** | SWC wrapper for TypeScript parsing |
| **perry-hir** | HIR types and data structures, plus AST→HIR lowering |
| **perry-transform** | IR passes (closure conversion, async lowering, inlining) |
| **perry-codegen** | LLVM-based native code generation |
| **perry-runtime** | Runtime: value.rs, object/, array/, string/, gc/, arena/, thread/ |
| **perry-stdlib** | Node.js API support (mysql2, redis, fetch, fastify, ws, etc.) |
| **perry-ui** / **perry-ui-macos** / **perry-ui-ios** / **perry-ui-tvos** | Native UI (AppKit/UIKit) |

## NaN-Boxing

Perry uses NaN-boxing to represent JavaScript values in 64 bits (`perry-runtime/src/value.rs`):

```
TAG_UNDEFINED = 0x7FFC_0000_0000_0001    BIGINT_TAG  = 0x7FFA (lower 48 = ptr)
TAG_NULL      = 0x7FFC_0000_0000_0002    POINTER_TAG = 0x7FFD (lower 48 = ptr)
TAG_FALSE     = 0x7FFC_0000_0000_0003    INT32_TAG   = 0x7FFE (lower 32 = int)
TAG_TRUE      = 0x7FFC_0000_0000_0004    STRING_TAG  = 0x7FFF (lower 48 = ptr)
```

Key functions: `js_nanbox_string/pointer/bigint`, `js_nanbox_get_pointer`, `js_get_string_pointer_unified`, `js_jsvalue_to_string`, `js_is_truthy`

**Module-level variables**: uniform NaN-boxed doubles in `@perry_global_<mod>__<id>` LLVM globals, all registered as GC roots before module init (marked AND rewritten on evacuation). The old F64-strings/raw-I64-arrays split and `module_var_data_ids` no longer exist (a stale comment survives in `perry-transform/src/inline/mod.rs`).

## Garbage Collection

The current collector source of truth is `docs/src/internals/garbage-collector.md`. Implementation lives in the `crates/perry-runtime/src/gc/` and `arena/` module trees. Native RS4GC roots are the target-aware default where the runtime can walk frames; shadow frames are the fallback on unsupported targets. The conservative native-stack scan is diagnostic-only by default (`Auto` resolves to `SkipDisabled`).

**Escape hatches**: `PERRY_GEN_GC=0`/`off`/`false` reverts to full mark-sweep (bisection only). #7611 deleted the ambient evacuation-policy veto after it moved 0 of 96 gc-ratchet cells and could silently disarm the #7154 stress instrument (`PERRY_GC_SCHEDULE_SEED`). Policy evacuation is unconditional now except on budgeted low-pause cycles, which has a behavioural test. `PERRY_GC_FORCE_EVACUATE=1` stress-copies every marked non-pinned nursery object when generated write barriers are active, **including on the explicit `gc()` path** — since #6946 a manual `gc()` under this knob runs an evacuating minor before its full mark-sweep, instead of a full sweep that moved nothing. `PERRY_GC_VERIFY_EVACUATION=1` panics if any mutable live slot still points at a forwarded nursery object after an evacuation/rewrite cycle. `PERRY_WRITE_BARRIERS=0`/`off`/`false` disables codegen-emitted write barriers at compile time and runtime exact helper barriers at runtime for benchmark/debug bisection; unset, `=1`/`on`/`true` keep barriers enabled. `PERRY_GC_DIAG=1` prints per-cycle diagnostics, including evacuation-policy decisions for considered cycles and `barriers_inactive` skips.

### Rooting-bug instruments (#7154 family) — what each knob ACTUALLY gates

A "GC value live but not rooted across a collection point" bug is invisible at collection time: there is nothing for the collector to find. It surfaces one or more cycles later, in a different function, as `TypeError: value is not a function`. These knobs exist to collapse that latency. **All default-off; every knob's OFF state is asserted — the quarantine's in `gc/tests/fromspace_protect.rs`, the schedule's in `gc/tests/schedule.rs`** (`…_DEPTH` and `…_RATE` are magnitudes, not modes, so their floors, defaults and clamping are asserted instead). The instruments are **sabotage-tested**, not merely exercised: `quarantine_catches_a_planted_stale_from_space_deref` plants a #7184/#7192-shaped stale from-space pointer and asserts the instrument distinguishes it from the live object that would otherwise be recycled into those bytes — so a green protected run means the detector works, not that nothing was tried.

| knob | gates EXACTLY | does NOT |
|---|---|---|
| `PERRY_GC_PROTECT_FROMSPACE=1` (or `poison`) | the from-space reset performed by the **copying minor** (`arena::copying_reset_from_spaces_and_flip`). Retired Eden + active-survivor blocks are detached into a bounded quarantine, poison-filled (`0xDEADBEEFBAADF0DE`, `obj_type = 0xDE`) and, at `=1`, `mprotect(PROT_NONE)`d. A stale deref then SIGSEGVs at the faulting instruction; the installed reporter names the address, the retiring minor, and the last-known object's `obj_type`/size, then restores `SIG_DFL` and re-faults so a core/debugger still sees the real site. `poison` skips `mprotect`. | change the non-moving minor's `arena_reset_empty_blocks`, the full mark-sweep's reclaim, old-gen defrag, or the malloc sweep. **A run with zero copying minors protects nothing** — check that `PERRY_GC_DIAG=1` prints a `[gc-fromspace-protect] retired_set=#N` line. |
| `PERRY_GC_PROTECT_FROMSPACE_DEPTH=N` (default 4) | how many retired page-sets stay quarantined. Evicted sets are restored to RW and **recycled back into Eden**, never `dealloc`'d, so footprint is bounded at `N × from-space bytes`. `0` is clamped to 1 — a depth of 0 would read as ON and protect nothing. **Raise this when a suspected bug does not fault**: a value can cross hundreds of collections between its last valid observation and its stale use (one per back-edge poll at `PERRY_GC_SCHEDULE_RATE=1`). #7154's `new C(…)` reproducer needs `800` — its constructor crosses 600 polls, so the default 4 misses it silently. | — |
| `PERRY_GC_FROMSPACE_SCAN_ABORT=1` | now **implies** `PERRY_GC_FROMSPACE_SCAN=1`. It used to be inert alone (the scan never ran, so nothing aborted, and the run reported success). | — |
| `PERRY_GC_SCHEDULE_SEED=<u64>` | seeded GC-schedule fuzzing — the collection schedule as a knob, from normal pacing up to a collection at every handled safepoint (`PERRY_GC_SCHEDULE_RATE=1`). Three things, exactly: (1) `js_gc_loop_safepoint` stops requiring `GC_SAFEPOINT_PENDING` before descending into `gc_safepoint_moving_minor`; (2) inside `gc_safepoint_moving_minor`, **past the entry guards**, a per-thread safepoint counter advances once per handled safepoint and, when `gc_budgeted_due_trigger()` reports nothing due, a minor runs anyway iff `splitmix64(splitmix64(seed) ^ counter) < threshold`; (3) `gc_force_evacuate_enabled()` becomes true, so survivors MOVE. **A value that does not parse as `u64` reads as OFF, not as seed 0.** The seed is printed at startup, from the process-exit teardown funnel every exit path routes through (`report_exit_summary`, on the collection-side-allocation release — perry's `_exit` paths never reach `atexit`, which is only a libc-return backstop), and on panic/SIGSEGV/SIGBUS/SIGABRT/SIGILL/SIGTRAP — the signal reporter chains to (and is re-layered on top of) the from-space quarantine's, so the two compose. Live-subject counters: `gc::gc_schedule_safepoints()` / `gc::gc_schedule_forced_collections()`. | bypass `gc_safepoint_moving_minor`'s entry guards — and a blocked safepoint deliberately does **not** tick the counter, so the ordinal sequence tracks the program's safepoints rather than its allocation state. The forced-evacuation implication is **unconditional** since #7611 deleted the ambient veto that could disarm it. Nor emit loop polls — those are a compile-time property (`PERRY_GC_MOVING_LOOP_POLLS`, default ON since #7721; a binary compiled with `=0` has none), and without them a seeded run only fires at event-loop boundaries and a compute-only loop never collects. Nor *suppress* pressure-driven collections — the rate is additional density, never less. Determinism is **per-thread**: the counter is thread-local, so a single-threaded program replays exactly, while a `perry/thread` program is only as reproducible as its OS scheduling. Say which you measured. |
| `PERRY_GC_SCHEDULE_RATE=<0..1>` (default `0.05`) | **only** the threshold `PERRY_GC_SCHEDULE_SEED`'s hash is compared against — the expected fraction of handled safepoints that collect. Out-of-range values clamp (a `2` reads as 1.0); unparseable and NaN fall back to the default. | do anything at all without a seed. It is inert alone. `=0` is an on-but-selects-nothing control (banner and reporters still install), `=1` collects at every handled safepoint — the maximum-density endpoint, where the seed stops mattering because every ordinal is selected whatever it hashes to. There is deliberately **no allocation-point level**: the alloc-point arm forces a conservative stack scan, which makes the copying minor ineligible, so an "every allocation" density would run non-moving minors and move nothing. |
| `PERRY_GC_SCHEDULE_ALLOC_KB=N` (default 4) | how much NEW nursery material must accumulate before a loop back-edge poll becomes a candidate the seed may select (#7728). A high-water mark measured AFTER each collection, not a delta, so a collection that frees nothing cannot loop. `0` restores the literal every-poll candidate set — right for a small fixture or a window that executes once, and far slower. | change the schedule itself: the seed still decides which candidates collect, so `(seed, counter)` replay is unaffected. Nor apply to microtask-pump safepoints — it paces the loop arm only. |

`PERRY_GC_SCHEDULE_SEED=<u64> PERRY_GC_PROTECT_FROMSPACE=1` together is the pairing that turns a #7154 bug into an immediate precise fault. Loop polls are default-ON since #7721, so in-loop coverage no longer needs a flag — check `loop_polls=` in the exit verdict rather than assuming. If a hunt needs maximum sensitivity on a small program, add `PERRY_GC_SCHEDULE_ALLOC_KB=0`.

### GC knob kill-policy (binding)

**Every GC env knob either has a required CI arm exercising its OFF state, or it is deleted after one release of soak.** At most one diagnostic-only knob may exist at a time, and it must be labelled untested.

This is not tidiness. An unexercised mode is a configuration nobody has verified, and this project has repeatedly paid for that:

- `PERRY_GC_FORCE_EVACUATE` was **inert** for every `gc()`-driven test — it is read only on the minor path, while `gc()` ran a full mark-sweep with a forced conservative scan (#6942/#6946). Months of "passes under evacuation" meant nothing. *(Both halves are closed: #7657 removed the forced scan, #6946 makes `gc()` under the knob run an evacuating minor first. Kept as the precedent it is.)*
- The matrix's `--pressure` knob **disabled the very path it was measuring** — the defer hard cap and the arena-trigger ceiling shared a formula and collapsed together, so the `default` arm ran zero copying minors on all 22 rows (#7024).
- `gc_incremental_enabled`'s doc said "EXPERIMENTAL — default OFF" eight lines above a body comment saying "DEFAULT ON" (#6987). A merge decision was made on the wrong one.

**A mode that still exists is a decision that hasn't been made.** When a knob's off-state stops being exercised, delete the off-state and the branch behind it — the losing mode should stop compiling, not linger as an untested configuration that a future bisect will trust.

## Threading (`perry/thread`)

Single-threaded by default. `perry/thread` provides:
- **`parallelMap(array, fn)`** / **`parallelFilter(array, fn)`** — data-parallel across all cores
- **`spawn(fn)`** — background OS thread, returns Promise

Values cross threads via `SerializedValue` deep-copy. Each thread has independent arena + GC. Results from `spawn` flow back via `PENDING_THREAD_RESULTS` queue, drained during `js_promise_run_microtasks()`.

## Native UI (`perry/ui`)

Declarative TypeScript compiles to AppKit/UIKit calls. Handle-based widget system (1-based i64 handles, NaN-boxed with POINTER_TAG). `--target ios-simulator`/`--target ios`/`--target tvos-simulator`/`--target tvos` for cross-compilation. **`release-packages.yml` no longer builds or ships tvOS/visionOS/watchOS artifacts**: they're Rust Tier-3 (no prebuilt std, needed `-Z build-std`, still unstable), and that leg is dropped from the release workflow entirely. The compiler itself still supports them: `perry compile --target tvos-simulator` (etc.) auto-rebuilds via `cargo +nightly -Z build-std` using whatever nightly the caller has installed (`crates/perry/src/commands/compile/link/build_and_run.rs`, `crates/perry/src/commands/compile/optimized_libs/driver.rs`), and `test.yml` still exercises that path for tvOS-sim doc-tests.

**To add a new widget** — change 4 places:
1. Runtime: `crates/perry-ui-macos/src/widgets/` — create widget, `register_widget(view)`
2. FFI: `crates/perry-ui-macos/src/lib.rs` — `#[no_mangle] pub extern "C" fn perry_ui_<widget>_create`
3. Codegen: `crates/perry-codegen/src/lower_call/native/native_ui_widgets_branch.rs` — declare extern + NativeMethodCall dispatch
4. HIR: `crates/perry-hir/src/lower/` — only if widget has instance methods

## Compiling npm Packages Natively (`perry.compilePackages`)

Configured in `package.json`:
```json
{ "perry": { "compilePackages": ["@noble/curves", "@noble/hashes"] } }
```
Bare imports resolve from the importer outward, matching Node/Bun. Every canonical
package root reached through `compilePackages` is tracked as a separate instance,
so nested versions compile with distinct module/symbol identities; multiple links
to the same physical root still deduplicate by canonical path.

## Known Limitations

- **No runtime type *validation***: declared TS types aren't enforced at runtime (a `string` param accepts a number, no throw). Annotations are mostly erased — the exception is `emitDecoratorMetadata`, which retains `design:type`/`design:paramtypes` from annotations on decorated members (see `docs/src/language/decorators.md`). Runtime type *discrimination* does exist: `typeof` via NaN-boxing tags, `instanceof` via class ID chain.
- **`SharedArrayBuffer` + `Atomics` cross-thread** (#4794 single-realm; #4913 Stage 2 cross-agent): the `Atomics` ops (`add`/`and`/`or`/`sub`/`xor`/`load`/`store`/`exchange`/`compareExchange`/`isLockFree`) match the spec on one thread. A `SharedArrayBuffer` captured into a `spawn`/`parallelMap` closure now **aliases the same physical bytes** across `perry/thread` agents (its backing is a process-global, never-freed allocation — `crate::shared_sab` — passed by reference, not deep-copied), and `Atomics.wait`/`notify`/`waitAsync` are **real**: `wait` parks the OS thread on a futex table keyed by the absolute slot address (`crate::atomics_futex`), `notify` wakes parked agents and returns the count, and `waitAsync` resolves its promise on a background thread when notified or on timeout. Caveat: only the `SharedArrayBuffer` itself shares — a typed-array *view* captured directly still deep-copies (build the view per-agent from the shared SAB). The agent-coordinated test262 cases (`$262.agent`) remain out of scope.

## Common Pitfalls & Patterns

### NaN-Boxing Mistakes
- **Double NaN-boxing**: If value is already F64, don't NaN-box again. Check `builder.func.dfg.value_type(val)`.
- **Wrong tag**: Strings=STRING_TAG, objects=POINTER_TAG, BigInt=BIGINT_TAG.
- **`as f64` vs `from_bits`**: `u64 as f64` is numeric conversion (WRONG). Use `f64::from_bits(u64)` to preserve bits.

### LLVM Type Mismatches
- Loop counter optimization produces i32 — always convert before passing to f64/i64 functions
- Constructor parameters always f64 (NaN-boxed) at signature level

### Async / Threading
- Thread-local arenas: JSValues from tokio workers invalid on main thread
- Use `spawn_for_promise_deferred()` — return raw Rust data, convert to JSValue on main thread
- Async closures: Promise pointer (I64) must be NaN-boxed with POINTER_TAG before returning as F64

### Cross-Module Issues
- ExternFuncRef values are NaN-boxed — use `js_nanbox_get_pointer` to extract
- Module init order: topological sort by import dependencies
- Optional params need `imported_func_param_counts` propagation through re-exports

### Closure Captures
- `collect_local_refs_expr()` must handle all expression types — catch-all silently skips refs
- Captured string/pointer values must be NaN-boxed before storing, not raw bitcast
- Loop counter i32 values: `fcvt_from_sint` to f64 before capture storage

### Handle-Based Dispatch
- TWO systems: `HANDLE_METHOD_DISPATCH` (methods) and `HANDLE_PROPERTY_DISPATCH` (properties)
- Both must be registered. Small pointer detection: value < 0x100000 = handle.

### objc2 v0.6 API
- `define_class!` with `#[unsafe(super(NSObject))]`, `msg_send!` returns `Retained` directly
- All AppKit constructors require `MainThreadMarker`

### Verifying a runtime change (read before you trust an A/B)
Build outputs are invisible to `git status`, so a clean tree tells you nothing about what you are actually linking. Three ways this bites:

- **Wrong build command → stale archive.** `perry-runtime`/`perry-stdlib` are `crate-type = ["rlib"]`; `libperry_{runtime,stdlib}.a` come from the `perry-runtime-static`/`perry-stdlib-static` wrappers (see Build Commands). `cargo build -p perry-runtime -p perry-stdlib` does **not** emit them, so `perry compile` links a stale `.a`: your fix looks like a no-op **and both arms of an A/B behave identically** (a vacuous "zero regressions"). Build `-p perry -p perry-runtime-static -p perry-stdlib-static`, pin `PERRY_RUNTIME_DIR`, and confirm the `.a` mtime moved *after* your edit. Keep the package set identical across builds/bisect hops — dropping `-p perry` changes cargo feature unification. (`run_parity_tests.sh` builds the wrappers itself, so gap runs are safe; hand-rolled `.ts` probes are not.)
- **A prebuilt binary in another worktree is not evidence about the commit it's checked out to** — it may be built from that worktree's WIP tree. Never use one as a bisect `good` endpoint or a perf baseline; build your own reference and *verify the good endpoint is actually good* first.
- **Check the harness's exit code, not a wrapper shell's.** A job piping the gap harness through `grep` can report exit 0 while the harness itself failed.

### CI gates that surprise people
- **2000-line-per-file cap** (`scripts/check_file_size.sh`) — run it before pushing; adding a long doc comment can trip it.
- **addr-class ratchet** (`scripts/addr_class_inventory.py`) — a file gaining a bare-address site fails `lint`.
- **`gap-suite` shards (formerly `conformance-smoke`)**: the PR/sweep tiers run the harness in `fast` mode (`PERRY_SKIP_BUILD=1`, one prebuilt release compiler, ~1.5 s/test); the full tier runs the 8-shard auto-optimize mode. Both compare against the same `test-parity/gap_snapshot.json`, so a divergence between them is an auto-optimize-specific finding. Before believing a red shard, A/B the named tests against a pristine `main` build; re-baseline only via the documented CI dispatch (`docs/src/testing/ci-tiers.md`).
- **Integration suites under `crates/*/tests/*.rs` run per-PR only when the diff names them** (`e2e-scoped`); the sweep/full tiers' `cargo test --workspace` is the backstop — a regression there lands on `main` and is attributed by sweep window. Prefer putting acceptance coverage in `cargo-test`-visible unit tests (#5960).
- **`perry-runtime`'s tests are not parallel-safe — run them `RUST_TEST_THREADS=1`.** They share process-global side tables (#1444), and ~180 readers are not required to take the clearing lock (see `gc::tests::global_sink_isolation`'s header; #7672 is converting them to `per_test_global!` one at a time). Every CI path already pins `RUST_TEST_THREADS=1`; a local `cargo test --workspace` does NOT, which is the whole gap. Measured on the default pool: 600 full-suite runs at `--test-threads=16` (load ~90) were clean, but at `--test-threads=64` (load ~115) **2 of 320 runs failed** — in `proxy::…numeric_write_guard…` and `array::element_shape::matrix_tests::matrix_delete_revokes`, neither related to the change under test. The tell is the message: these fail on their own *fixture precondition* ("fixture must start proven, or every verdict below is vacuous"), i.e. another thread wiped the global, not a real defect in the code under test. Chasing such a failure as if it were a regression is wasted days.

### ★ Four ways a gate can be unable to fail

All four look fine on the Actions page. None can turn a merge red. When adding or reviewing a gate, check all four — each has bitten this repo, three of them within one week:

1. **`continue-on-error: true`** — `gc-stress` carried it for months while being the only job covering GC correctness.
2. **Not in branch protection's required contexts** — `gc-stress` again. This is why #6925's `PERRY_PTR_SHAPE_LOCALS=0` regression landed visibly red and survived three merges. A job that reports failure without blocking is documentation, not a gate.
3. **`concurrency` with unconditional `cancel-in-progress`** — on a branch with a slow runner queue, every new merge cancels the previous run before it reaches a runner. `gc-ratchet` had three consecutive `main` runs cancelled, zero executed. Scope cancellation to `pull_request` and let `main` runs queue.
4. **The gate runs but its subject never did** — the most dangerous, because the job is genuinely green. `PERRY_GC_FORCE_EVACUATE` was inert for every `gc()`-driven test (#6942/#6946); the matrix's `--pressure` knob disabled the very path it was measuring (#7024); its `moved=` counter summed two different collectors, so a cell could pass having run zero copying minors (#7025). **A gate must assert its subject was live**, not merely that nothing threw — e.g. `copied_objects > 0` before a green verdict.

Corollary: a *new* gate has never been green, so promoting it to required immediately blocks every open PR. Run it once, then promote. Leaving that second step undone is how (2) happens.

### Known-weak areas (symptom is often not the bug)
- **Async-to-generator transform, body locals.** It boxes every body local into a shared mutable cell typed `Any`. Two consequences seen in the wild: per-iteration `let`/`const` bindings collapse for closures created in a loop, and computed numeric-key calls (`arr[i](x)`) lose their type proof and silently resolve by *method name*, evaporating the call.
- **Native base-class subclassing.** A native base's surface is installed at `super()` time and its parent edge lives in the class registry; keying any of that on a literal `extends` name loses it for fieldless classes, indirect subclasses, and class expressions.
- **Two prototype-resolution paths.** `CLASS_PROTOTYPE_OBJECTS` (synthetic: `Object.create`, plain-function ctors) vs `CLASS_DECL_PROTOTYPE_OBJECTS` (declared classes). `in`/`for…in` and `getPrototypeOf` have disagreed about the same chain.
- **Root-store dominance in codegen.** *A GC-managed value's root store must **dominate** every subsequent site that can collect.* Three ways it has broken, all shipped: the store's slot index fell outside the pushed shadow frame so `js_shadow_slot_bind` bounds-checked it into a silent no-op (#7184); the store was emitted in-frame but **after** a call that allocates (#7192); and the value lives in a plain `alloca_entry` that is neither a shadow slot nor a temp root, so the collector never rewrites it (`lower_call/new.rs`'s inline-ctor `this_slot`, closed by #7207; `--unrooted-allocas` is the detector for that shape, and its remaining hits are #7210's). All three present identically — a *rooted* slot holding a dangling pointer, surfacing cycles later as `TypeError: value is not a function` — and **none is visible to any runtime GC probe**, because at the moment of the collection there is nothing for the collector to find. That is why #7154's from-space scan only ever saw offenders whose targets had already died. The instrument is static: `scripts/gc_root_dominance_check.py` over `--trace llvm` output (`--self-test` proves it can still fail). Only bites where a back-edge poll is emitted, which is again the default (`PERRY_GC_MOVING_LOOP_POLLS`, kill switch `=0`) in every loop that can allocate — so a green default run does say something about this class, and `=0` is what makes it dark. **Full writeup, every known shape and how to check your work: `docs/src/internals/gc-rooting-invariant.md`.** The CI gate is `gc-root-dominance.yml` over `scripts/gc_root_dominance_corpus.sh`; known-remaining hits are named one-per-entry in `scripts/gc_root_dominance_allowlist.json` (an entry that matches nothing FAILS, so a fix must delete its entry), and that list is currently **empty** — every new hit is a red build.
- **A runtime-side cache of a raw heap pointer is a GC root, and the static checker cannot see it.** `scripts/gc_root_dominance_check.py` reads emitted LLVM IR, so a thread-local or side table holding a `*mut` into the heap is structurally invisible to it — the runtime instruments above are the only detector, and they go at the workload *before* you grind the static checker's tail. Two tells. An unrooted *register* goes bad only when a collection lands in its window, so it is intermittent; an unrooted *cache* goes bad at collection #0 and stays bad, so **a perfectly reproducible GC bug means a table, not a register**. And the registry is `gc_register_mutable_root_scanner` in `crates/perry-runtime/src/gc/roots.rs`: when you add a cache of a heap pointer, register it there in the same commit. (Don't restate the population size here — this file carried "~55 entries" long after it passed 120, in two places, of which one got fixed. `scripts/gc_runtime_root_holders.py` prints the current count and enforces a floor.) **The population is now enumerated and gated**: `scripts/gc_runtime_root_holders.py` (in `lint`) lists every `static`/`thread_local!` in `perry-runtime`/`perry-stdlib` whose type can hold a heap pointer, computes which ones a registered scanner actually reaches (call-graph walk, so accessors like `cp_live_lock()` count), and requires a written verdict in `scripts/gc_runtime_root_holders.json` for the rest — a new unclassified holder fails, and so does a stale entry, so a fix must delete its own exemption. Worked examples: `changelog.d/7219-registry-gc-unrooted-caches.md`, `changelog.d/7239-gc-unrooted-runtime-caches.md`.
