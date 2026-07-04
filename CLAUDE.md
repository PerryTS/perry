# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.1232


## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`, 258 tests). Compared byte-for-byte against `node --experimental-strip-types`. Run via `./scripts/run_gap_tests.sh` (a thin wrapper over `run_parity_tests.sh --filter test_gap_` that builds the compiler itself and gates on no new untriaged failures).

**Last full sweep:** run `./run_parity_tests.sh` for the current snapshot. The umbrella tracker is #793 (Node.js + TypeScript compatibility roadmap); the previously-cited #447–#452 batch closed on 2026-05-04. Currently-open trackers worth knowing about:

- **Effect framework end-to-end (#321)** — `#684` (Schema.ts ~310th-init `(number).slice` regression) and `#809` (object-literal computed-keys + cross-module spread) are the live HashRing/Schema blockers.
- **Async context** — `AsyncLocalStorage` (real tracking across `await`/microtasks/timers, `#788`) and `async_hooks.createHook` (real lifecycle + asyncId, `#789`) both landed (closed 2026-05-16); these are no longer stubs.
- **Compile-as-package** — `#348` (ink TUI end-to-end), `#488/#489` (Drizzle + MySQL), `#678` (linker emits native callsites for V8-fallback modules).
- **Test/CI mechanics** — `#794` (per-category parity thresholds), `#796` (gap-suite output truncation + O(n²) `normalize_output`), `#812` (42-module behavioral matrix), `#806/#807/#808` (test harnesses for mixins / async context / ≥300-init scale).
- **Skip-list audit** — `#797` covers `test-parity/known_failures.json` provenance (issue # + date per entry).

**Known categorical gaps**: `console.dir`/`console.group*` formatting, lone surrogate handling (WTF-8). (Lookbehind regex is NOT a gap anymore: `perry-runtime/src/regex.rs` falls back from the `regex` crate to `fancy-regex` for lookbehind/backreferences, with capture-group translation and replacement expansion.)

## Workflow Requirements

**Default flow is PR-based.** `main` is protected: pushes require a pull request, CI must pass (`lint`, `cargo-test`, `api-docs-drift`, `security-audit`), and only squash or rebase merges are allowed (no merge commits, linear history enforced). `parity` and `compile-smoke` are gated to tag pushes only (v0.5.1018) — they no longer run on PRs but still gate the release-packages.yml publish step. Admins can bypass for hotfixes/version bumps, but the standard path is:

1. Branch from `main`, push, open a PR.
2. Wait for required checks to go green.
3. Squash- or rebase-merge. The PR branch auto-deletes on merge.

**For every change that lands on `main`** (whether via PR or admin bypass):

1. **Bump version**: Increment patch in `[workspace.package].version` in `Cargo.toml` and the `**Current Version:**` line above. That is the ONLY metadata edit CLAUDE.md needs.
2. **Add changelog entry**: Prepend a new `## v0.5.x — <one-line summary>` block at the top of `CHANGELOG.md`. Detail can go below in the same block — long-form root-cause writeups, file paths, validation notes, etc. all belong here, NOT in CLAUDE.md.
3. **Commit changes**: Include code, `Cargo.toml`/`Cargo.lock`, `CLAUDE.md` (version bump only), and `CHANGELOG.md` updates together.

**Do not write changelog entries into CLAUDE.md.** This file is for orientation (architecture, common pitfalls, build commands). Per-version history lives in `CHANGELOG.md` so CLAUDE.md stays small and stable across context loads.

### External contributor PRs

PRs from outside contributors should **not** touch `[workspace.package] version` in `Cargo.toml`, the `**Current Version:**` line in `CLAUDE.md`, or `CHANGELOG.md`. The maintainer bumps the version and writes the changelog entry at merge time — usually by rebasing the PR branch and amending. This avoids the patch-version collisions that happen when Perry's `main` ships several commits while a PR is in review (each on-main commit bumps the version; a PR that bumped to the same patch on day 1 is already behind by merge day). Contributors just write code; let the maintainer fold in the metadata last.

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
cargo test --release --workspace \
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
| **perry-types** | Type system definitions |
| **perry-hir** | HIR data structures (`ir.rs`) and AST→HIR lowering (`lower.rs`) |
| **perry-transform** | IR passes (closure conversion, async lowering, inlining) |
| **perry-codegen** | LLVM-based native code generation |
| **perry-runtime** | Runtime: value.rs, object.rs, array.rs, string.rs, gc.rs, arena.rs, thread.rs |
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

Generational mark-sweep GC in `crates/perry-runtime/src/gc.rs` (default since v0.5.237 / Phase D). Two regions in the per-thread arena: nursery (`ARENA`, fills with new allocations, swept on minor GC) and old-gen (`OLD_ARENA`, holds tenured/evacuated objects). Precise shadow-stack roots + ~55 registered side-table scanners (`gc/mod.rs:298+`); a conservative stack scan exists but production mode resolves to SkipDisabled, so liveness rests on codegen shadow-stack spilling plus `RuntimeHandleScope` in runtime helpers. Write barriers populate a remembered set so minor GC can avoid retracing the old-gen. Two-bit aging (`HAS_SURVIVED` / `TENURED`) promotes nursery survivors after 2 minor cycles; the C4b evacuation policy moves non-pinned tenured objects into old-gen with full reference rewriting only when generated write barriers are active and nursery/RSS pressure plus measured movable candidates justify the work. Idle nursery blocks observed empty for 2 GC cycles are `dealloc`'d back to the OS (C4b-δ, v0.5.235), and the next-trigger calc is hard-capped at the initial threshold (64 MB) so >90%-freed step-doubling can't blow up peak occupancy (C4b-δ-tune, v0.5.236). Triggers on arena block allocation (1 MB blocks since v0.5.196), malloc count threshold, or explicit `gc()` call. 8-byte GcHeader per allocation.

**Escape hatches**: `PERRY_GEN_GC=0`/`off`/`false` reverts to full mark-sweep (bisection only). `PERRY_GEN_GC_EVACUATE=0`/`off`/`false` disables policy evacuation; `=1`/`on`/`true` is accepted as auto-policy allowed, not unconditional evacuation. `PERRY_GC_FORCE_EVACUATE=1` stress-copies every marked non-pinned nursery object only when generated write barriers are active and policy evacuation is allowed. `PERRY_GC_VERIFY_EVACUATION=1` panics if any mutable live slot still points at a forwarded nursery object after an evacuation/rewrite cycle. `PERRY_WRITE_BARRIERS=0`/`off`/`false` disables codegen-emitted write barriers at compile time and runtime exact helper barriers at runtime for benchmark/debug bisection; unset, `=1`/`on`/`true` keep barriers enabled. `PERRY_GC_DIAG=1` prints per-cycle diagnostics, including evacuation-policy decisions for considered cycles and `barriers_inactive` skips.

## Threading (`perry/thread`)

Single-threaded by default. `perry/thread` provides:
- **`parallelMap(array, fn)`** / **`parallelFilter(array, fn)`** — data-parallel across all cores
- **`spawn(fn)`** — background OS thread, returns Promise

Values cross threads via `SerializedValue` deep-copy. Each thread has independent arena + GC. Results from `spawn` flow back via `PENDING_THREAD_RESULTS` queue, drained during `js_promise_run_microtasks()`.

## Native UI (`perry/ui`)

Declarative TypeScript compiles to AppKit/UIKit calls. Handle-based widget system (1-based i64 handles, NaN-boxed with POINTER_TAG). `--target ios-simulator`/`--target ios`/`--target tvos-simulator`/`--target tvos` for cross-compilation.

**To add a new widget** — change 4 places:
1. Runtime: `crates/perry-ui-macos/src/widgets/` — create widget, `register_widget(view)`
2. FFI: `crates/perry-ui-macos/src/lib.rs` — `#[no_mangle] pub extern "C" fn perry_ui_<widget>_create`
3. Codegen: `crates/perry-codegen/src/codegen.rs` — declare extern + NativeMethodCall dispatch
4. HIR: `crates/perry-hir/src/lower.rs` — only if widget has instance methods

## Compiling npm Packages Natively (`perry.compilePackages`)

Configured in `package.json`:
```json
{ "perry": { "compilePackages": ["@noble/curves", "@noble/hashes"] } }
```
First-resolved directory cached in `compile_package_dirs`; subsequent imports redirect to the same copy (dedup).

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

## Recent Changes

Per-version entries live in CHANGELOG.md.

**Do not add changelog entries to this file.** Bump only the `**Current Version:**` line above when you ship a release; everything else goes in CHANGELOG.md as a new `## v0.5.x — ...` block at the top.
