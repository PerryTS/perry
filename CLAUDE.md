# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.370

## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`, 28 tests). Compared byte-for-byte against `node --experimental-strip-types`. Run via `/tmp/run_gap_tests.sh` after `cargo build --release -p perry-runtime -p perry-stdlib -p perry`.

**Last sweep:** **26/28 passing** (re-measured v0.5.319 after the SSO unboxing fix transitively flipped 8 previously-failing tests: `array_methods`, `async_advanced`, `error_extensions`, `fetch_response`, `global_apis`, `map_set_extended`, `object_methods`, `string_methods`). Known failing: `console_methods` (ci-env diff), `typed_arrays` (categorical gap). Run via `/tmp/run_gap_tests.sh` after full rebuild.

**Known categorical gaps**: lookbehind regex (Rust `regex` crate), `console.dir`/`console.group*` formatting, lone surrogate handling (WTF-8).

## Workflow Requirements

**IMPORTANT:** Follow these practices for every code change made directly on `main` (maintainer workflow):

1. **Update CLAUDE.md**: Add 1-2 line entry in "Recent Changes" for new features/fixes
2. **Increment Version**: Bump patch version (e.g., 0.5.48 → 0.5.49)
3. **Commit Changes**: Include code changes and CLAUDE.md updates together

### External contributor PRs

PRs from outside contributors should **not** touch `[workspace.package] version` in `Cargo.toml`, the `**Current Version:**` line in `CLAUDE.md`, or add a "Recent Changes" entry. The maintainer bumps the version and writes the changelog entry at merge time — usually by rebasing the PR branch and amending. This avoids the patch-version collisions that happen when Perry's `main` ships several commits while a PR is in review (each on-main commit bumps the version; a PR that bumped to the same patch on day 1 is already behind by merge day). Contributors just write code; let the maintainer fold in the metadata last.

## Build Commands

```bash
cargo build --release                          # Build all crates
cargo build --release -p perry-runtime -p perry-stdlib  # Rebuild runtime (MUST rebuild stdlib too!)
cargo test --workspace --exclude perry-ui-ios  # Run tests (exclude iOS on macOS host)
cargo run --release -- file.ts -o output && ./output    # Compile and run TypeScript
cargo run --release -- file.ts --print-hir              # Debug: print HIR
```

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
| **perry-jsruntime** | JavaScript interop via QuickJS |

## NaN-Boxing

Perry uses NaN-boxing to represent JavaScript values in 64 bits (`perry-runtime/src/value.rs`):

```
TAG_UNDEFINED = 0x7FFC_0000_0000_0001    BIGINT_TAG  = 0x7FFA (lower 48 = ptr)
TAG_NULL      = 0x7FFC_0000_0000_0002    POINTER_TAG = 0x7FFD (lower 48 = ptr)
TAG_FALSE     = 0x7FFC_0000_0000_0003    INT32_TAG   = 0x7FFE (lower 32 = int)
TAG_TRUE      = 0x7FFC_0000_0000_0004    STRING_TAG  = 0x7FFF (lower 48 = ptr)
```

Key functions: `js_nanbox_string/pointer/bigint`, `js_nanbox_get_pointer`, `js_get_string_pointer_unified`, `js_jsvalue_to_string`, `js_is_truthy`

**Module-level variables**: Strings stored as F64 (NaN-boxed), Arrays/Objects as I64 (raw pointers). Access via `module_var_data_ids`.

## Garbage Collection

Generational mark-sweep GC in `crates/perry-runtime/src/gc.rs` (default since v0.5.237 / Phase D). Two regions in the per-thread arena: nursery (`ARENA`, fills with new allocations, swept on minor GC) and old-gen (`OLD_ARENA`, holds tenured/evacuated objects). Conservative stack scan + precise shadow-stack roots + 9 registered scanners. Write barriers populate a remembered set so minor GC can avoid retracing the old-gen. Two-bit aging (`HAS_SURVIVED` / `TENURED`) promotes nursery survivors after 2 minor cycles; the C4b evacuation pass moves non-pinned tenured objects into old-gen with full reference rewriting. Idle nursery blocks observed empty for 2 GC cycles are `dealloc`'d back to the OS (C4b-δ, v0.5.235), and the next-trigger calc is hard-capped at the initial threshold (64 MB) so >90%-freed step-doubling can't blow up peak occupancy (C4b-δ-tune, v0.5.236). Triggers on arena block allocation (1 MB blocks since v0.5.196), malloc count threshold, or explicit `gc()` call. 8-byte GcHeader per allocation.

**Escape hatches**: `PERRY_GEN_GC=0`/`off`/`false` reverts to full mark-sweep (bisection only). `PERRY_GEN_GC_EVACUATE=1` enables the copying evacuation pass (default OFF — complete and correctness-safe but adds work that's a no-op on workloads where nothing tenures). `PERRY_WRITE_BARRIERS=1` opts into codegen-emitted write barriers (default OFF — barrier emission has its own perf cost; the runtime barrier always exists). `PERRY_GC_DIAG=1` prints per-cycle diagnostics.

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

- **No runtime type checking**: Types erased at compile time. `typeof` via NaN-boxing tags. `instanceof` via class ID chain.
- **No shared mutable state across threads**: No `SharedArrayBuffer` or `Atomics`.

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

Keep entries to 1-2 lines max. Full details in CHANGELOG.md.

- **v0.5.370** — Closes #255: perry-jsruntime no longer panics with `RefCell already borrowed` (then `active scope can't be dropped`) when a Perry closure passed to a JS-imported function (via `JsCreateCallback`, landed in #248 Phase 2B / v0.5.369) reads a property from a JS object passed in as a callback argument. The user's exact `gp.use((ctx) => log(ctx.deltaTime))` shape from issue #248 — which had been working around `gp.use(() => counter++)` only — now runs end-to-end. **Two cascading fixes**: (1) `with_runtime` is now re-entrancy-safe via a thread-local `REENTRY_PTR: Cell<*mut JsRuntimeState>` that the outer `with_runtime` body stashes on entry. Inner re-entrant calls (V8 trampoline → Perry callback → `js_get_property` → `with_runtime`) detect the stash and reuse the outer's `&mut JsRuntimeState` instead of trying to acquire a second `RefCell::borrow_mut`. A Drop guard clears the pointer on normal return AND on panic-unwind. `ensure_runtime_initialized` short-circuits when the stash is non-null (re-entrant code can't initialize a runtime that's already initialized). (2) But that alone surfaced a SECOND panic from V8's scope tracker: `state.runtime.handle_scope()` inside the re-entrant FFI conflicts with the trampoline's own active scope (V8 internally has a scope stack and rejects out-of-order Drops). **Fix**: the trampoline now stashes its `&mut HandleScope` in a second thread-local `REENTRY_SCOPE_PTR`, and `js_handle_object_get_property` checks for it and reuses the trampoline's existing scope instead of calling `state.runtime.handle_scope()`. Public helpers `stash_trampoline_scope()` (returns a `TrampolineScopeGuard` for LIFO Drop) and `try_trampoline_scope()` (unsafe — returns `Option<&'a mut HandleScope<'a>>`, lifetime is the caller's responsibility, valid only while the trampoline frame is on the stack). The body of `js_handle_object_get_property` factored into `get_property_with_scope(scope, ...)` so both the normal path (creates a new scope) and the trampoline-reuse path share the same logic. **Scope of the fix**: `js_handle_object_get_property` is the single FFI most commonly hit from inside callbacks (any `obj.field` read on a JS-supplied arg). Other re-entrant FFIs (`js_set_property`, `js_call_method` nested, `js_handle_array_get`, `js_handle_array_length`) use the same `state.runtime.handle_scope()` pattern and remain panic-prone if called from inside callbacks — refactoring them to use `try_trampoline_scope()` is mechanical (extract body into `_with_scope` helper, add the trampoline-stash branch) and tracked as a follow-up. **Safety analysis**: the raw-pointer escape hatch is the standard callback-driven re-entrancy pattern. The outer `&mut state` reference is paused on the call stack while V8 → trampoline → Perry callback → inner FFI runs; the inner reference is only live during that window; they never alias in time. Strictly UB by Rust's aliasing rules, but it's what every callback-driven library that holds state ends up doing in some form (deno_core uses an OpState handoff at await points instead, but our synchronous re-entry case can't suspend). Documented as `unsafe fn try_trampoline_scope`. New regression test `test-files/test_issue_255_jsruntime_reentrancy.ts` + fixture `test-files/fixtures/issue_255_jsmod.js` exercises: single property read inside callback (the user's #248 repro), multi-property + nested object property read (`ev.target.id` exercises 2-level re-entrancy), same callback fired twice (verifies REENTRY_SCOPE_PTR stash/restore guard), captured-variable mutation + property read combined. All print correctly; pre-fix the first case panicked. **Verified**: cargo build --release -p perry-runtime -p perry-stdlib -p perry-jsruntime -p perry clean; cargo test --release -p perry-jsruntime --lib 4/0; gap tests 25/28 = baseline; #248 Phase 2B regression test (test_issue_248_phase2b_js_callback.ts) still passes (`call0 counter: 1`, `call1 received: 42`, `call2 sum: 30`, `call3 sum: 6`, `callTwice count: 2`).
- **v0.5.369** — Closes #248 (Phase 2B): wire `JsCreateCallback` end-to-end so Perry closures passed to JS-imported functions (`arr.forEach(cb)`, `gameLoop.use(handler)`, etc.) actually fire. Phase 2A (v0.5.368) bailed at codegen for this variant because the runtime FFI `js_create_callback(func_ptr, closure_env, param_count)` expects `func_ptr` to have signature `(closure_env: i64, args_ptr: *const f64, args_len: i64) -> f64` (the `native_callback_trampoline` contract at `crates/perry-jsruntime/src/interop.rs:993`), but Perry closure bodies have `(closure_ptr, arg0, arg1, ...)` per arity — no direct call-compatible mapping. **Phase 2B bridge**: new runtime helper `js_closure_call_array(closure_env, args_ptr, args_len) -> f64` in `crates/perry-runtime/src/closure.rs` mirrors the existing `js_native_call_value` dispatch but takes `closure_env: i64` (raw `*const ClosureHeader`) as its first arg instead of an f64 NaN-boxed value, so the SysV-x64 / Win64 first-arg register lands in the integer slot (rdi/rcx) rather than the float slot (xmm0) — matching the trampoline's `extern "C" fn(i64, *const f64, i64) -> f64` expectation. Body switches on `args_len` and calls the right `js_closure_callN` (0..16). **JsCreateCallback codegen** in `crates/perry-codegen/src/expr.rs` now lowers to: lower the closure expression, unbox to i64 closure pointer, `ptrtoint @js_closure_call_array to i64` for the trampoline target, call `js_create_callback(func_addr, closure_ptr, param_count)`. Result is a NaN-boxed JS handle (V8-handle tag 0x7FFB) that JS code can call like any other JS function. **Closure-collector update** in `crates/perry-codegen/src/collectors.rs`: 8 new arms for the `Js*` HIR family (JsLoadModule, JsGetExport, JsCallFunction, JsCallMethod, JsGetProperty, JsSetProperty, JsNew, JsNewFromHandle, JsCreateCallback) so closures nested inside JS-interop expressions don't fall through the `_ => {}` catch-all and end up referenced via `js_closure_alloc(@perry_closure_*)` with their bodies never defined (clang error: "use of undefined value" — same class of bug as Tier 1.1). **Int32 unbox at the dispatch boundary**: V8 NaN-boxes JS integers with `INT32_TAG=0x7FFE` (perry-jsruntime's `bridge.rs:215`); Perry's closure-body arithmetic uses raw `fadd` / `fsub` / `fmul` on f64 inputs and assumes plain doubles, not tagged values. Pre-fix `cb(10, 20)` from JS into a Perry closure `(a, b) => a + b` returned 10 instead of 30 because `fadd(10.0, NaN-boxed-int32-20)` produces a NaN whose payload `console.log`'s tag-aware unbox decoded as 10 (one of the operands' int32). The dispatch helper now unboxes any INT32_TAG-tagged f64 to a plain double before passing to `js_closure_callN`. New regression test `test-files/test_issue_248_phase2b_js_callback.ts` + fixture `test-files/fixtures/issue_248_phase2b_jsmod.js` exercises 0/1/2/3-arg callbacks with mutable captures plus same-callback-fired-twice (using a method-call dispatcher object so `js_call_method` forces perry-jsruntime to win Mach-O link resolution and V8 actually runs end-to-end). All 5 cases now print correctly: `call0 counter: 1`, `call1 received: 42`, `call2 sum: 30`, `call3 sum: 6`, `callTwice count: 2`. The user's exact `gp.use((ctx) => {...})` shape from `/tmp/issue248/test_callback_simple.ts` also now compiles + runs end-to-end (V8 fires the closure, capture mutations propagate back to outer Perry scope, `done, final counter = 1`). **Known follow-up**: callbacks that read fields from JS-supplied object args (`ctx.deltaTime` inside `gp.use((ctx) => log(ctx.deltaTime))`) trigger a pre-existing perry-jsruntime re-entrancy panic (`RefCell already borrowed` at `crates/perry-jsruntime/src/lib.rs:123`) — V8's trampoline holds the `JsRuntimeState` borrow while invoking the Perry callback, and the callback's `js_get_property` re-enters `with_runtime` on the same borrow. Orthogonal to #248's codegen scope. **Verified**: cargo build --release -p perry-runtime -p perry-stdlib -p perry-jsruntime -p perry clean (note: rebuilding perry-jsruntime is necessary after a perry-runtime change because Cargo bundles the perry-runtime CGU inside `libperry_jsruntime.a` for link-time symbol resolution; without the rebuild the bundled CGU has the OLD source); cargo test --release -p perry-codegen --lib 22/0; gap tests 25/28 = baseline (the 3 failing — `array_methods` / `console_methods` / `typed_arrays` — pre-existing categorical/CI gaps unrelated to this change).
- **v0.5.368** — Closes #248: codegen for `arr.push(...src)` and the V8 / perry-jsruntime interop expression family. **Phase 1 (ArrayPushSpread)**: `arr.push(...src)` was rejected by the LLVM backend with `expression ArrayPushSpread not yet supported`. HIR has lowered the variant since v0.5.x (`crates/perry-hir/src/lower/expr_call.rs:2077`); only the codegen arm in `crates/perry-codegen/src/expr.rs` was missing — WASM (`crates/perry-codegen-wasm/src/emit.rs:5441`) and analysis helpers (`collectors.rs:1218`, `walker.rs:786`, `analysis.rs:34`) all knew about it. Fix is a single new arm mirroring `Expr::ArrayPush` at line 3016 (same three receiver storage cases — LocalGet / boxed-captured / plain — and the same realloc-aware writeback). No new runtime helper needed: `js_array_concat(dst, src)` already existed at `crates/perry-runtime/src/array.rs:1011`, comment-as-spec'd "reserved for the internal push-spread desugaring path"; Set sources work transparently via the SET_REGISTRY check inside `js_array_concat`. **Phase 2 (V8 interop, 8 new arms)**: the LLVM backend bailed for **JsLoadModule, JsGetExport, JsCallFunction, JsCallMethod, JsGetProperty, JsSetProperty, JsNew, JsNewFromHandle** — the HIR family `perry-hir/src/js_transform.rs::transform_js_imports` produces whenever a `.ts` entry imports from a `.js` module the resolver classifies as JS-runtime-loaded (`crates/perry/src/commands/compile/collect_modules.rs:73`, extension-driven). Pre-fix the user's `bun i @codehz/pipeline` repro bombed at codegen with `JsCallFunction not yet supported`. New arms call into the existing perry-jsruntime FFI surface: `js_load_module(path_ptr, path_len) -> u64`, `js_get_export/get_property/set_property/call_function/call_method/new_instance/new_from_handle` etc. — all eight already declared in `runtime_decls.rs` except `js_call_method` (added here at line 1631 with signature `DOUBLE, &[DOUBLE, I64, I64, I64, I64]`). New shared helper `lower_js_args_array(ctx, lowered_args) -> (ptr, len)` marshals already-lowered NaN-boxed args into a stack alloca'd `[N x double]` via the issue-#167 `alloca_entry_array` pattern (hoisted to function entry block); empty input returns `("null", "0")` for the FFI's null-pointer fallback. **Module handle representation**: V8 module ids are u64; codegen returns them as f64 via `bitcast_i64_to_double` to fit `lower_expr`'s return-type contract, then consumers bitcast back to i64 before passing to the runtime. **JS value handles** are NaN-boxed f64 with V8-handle tag 0x7FFB — handled internally by perry-jsruntime's `v8_to_native` / `native_to_v8` helpers; codegen treats them as opaque doubles. **Runtime bootstrap**: new `needs_js_runtime: bool` field on `CrossModuleCtx` (threaded from `CompileOptions::needs_js_runtime`, originally set in `collect_modules.rs:105` when any `.js` module enters `ctx.js_modules`), wired into `compile_module_entry` so the entry main's prelude calls `js_runtime_init()` between `js_gc_init` and user code. Without this, every `js_load_module` site bailed at the runtime with `[js_load_module] no JS runtime state!`. **`JsCreateCallback` deliberately deferred** to Phase 2B: the runtime FFI `js_create_callback(func_ptr, closure_env, param_count)` expects `func_ptr` to have signature `(closure_env: i64, args_ptr: *const f64, args_len: i64) -> f64` (see the `native_callback_trampoline` in `crates/perry-jsruntime/src/interop.rs:993`), but Perry closure bodies have `(closure_ptr, arg0, arg1, ...)` per arity — there's no direct call-compatible mapping. Wiring this needs either codegen-emitted per-arity adapter thunks or a runtime-side closure-array dispatcher; for now the arm bails with a clear message so users see exactly what's blocked. The user's exact `pipeline()` repro at `/tmp/issue248/test.ts` now compiles + links + runs (exit 0). **Regression tests**: `test-files/test_issue_248_array_push_spread.ts` (10 cases — number/string/object arrays, empty src/dst, array-literal spread, chained push-spread, post-spread `.indexOf` + `.length`, push-spread inside loop forcing realloc past the 16-cap, mixed `push` + `push-spread` — all byte-for-byte against `node --experimental-strip-types`), plus `test-files/test_issue_248_phase2_js_interop.ts` + fixture `test-files/fixtures/issue_248_jsmod.js` exercising JsLoadModule + JsCallFunction (compile + link + clean exit). **Verified**: cargo build --release -p perry-runtime -p perry-stdlib -p perry-jsruntime -p perry clean; cargo test --release -p perry-codegen --lib 22/0; gap tests 25/28 = baseline. Bumped 0.5.366 → 0.5.368 above origin's parallel-track 0.5.366 (HarmonyOS SDK fix #250) + 0.5.367 (HarmonyOS HAP bundler #252) per the merge-collision precedent. PR #251.
- **v0.5.369** — HarmonyOS PR B.4 + B.5 + B.6 squash-equivalent: cherry-picks `3042563a` + `b01653f6` + `41d597c0` (originally v0.5.127 / v0.5.128 / v0.5.129) from the `harmony-os` branch — the audit-driven fixes the original branch made AFTER its own first emulator run. End-to-end `hdc install` is now achievable (modulo cert + bundle-name match). **B.5 (v0.5.128) — DevEco 6.x SDK + ets-loader replacement**: most of B.5's compile.rs work already on main via the v0.5.366 fast-follow (DevEco app-bundle SDK probe + macOS framework leak fix); cherry-pick fold-in here is just the NEW hunks: (a) extends the `is_harmonyos` linker arm in `compile/link.rs` with OHOS runtime libs `-Wl,--allow-multiple-definition -lm -lpthread -ldl -lace_napi.z`. `libace_napi.z.so` is what ArkTS exposes for `napi_module_register` / `napi_create_*` (consumed by `perry-runtime/src/ohos_napi.rs`); OHOS naming convention is `<name>.z.so` and `-l` strips `lib`+`.so` but NOT the middle `.z`, so `-lace_napi.z` is the deliberate spelling. (b) Skip BSD strip on harmonyos targets — macOS strip emits a noisy `non-object and non-archive file` warning on ELF binaries. (c) `crates/perry/src/commands/harmonyos_hap.rs` rewritten to skip the ets-loader Node/rollup pipeline entirely and shell out to `es2abc --extension ts --module --merge-abc` directly — the harmony-os branch found ets-loader needs ~15 env vars (aceModuleRoot, aceModuleBuild, aceModuleJsonPath, aceProfilePath, compileMode=moduleJson, plus a full DevEco build-profile.json5); synthesizing all of that is effectively re-implementing hvigor. The Phase-1 ArkTS shim is plain TypeScript (no `@Entry`/`@Component`/`struct` decorators yet) so es2abc accepts it via the `--extension ts` flag. HAPs now ship a single merged `ets/modules.abc` instead of per-file .abc. PR C reintroduces ets-loader once the TS→ArkUI emitter produces real ArkUI decorators. (d) `EntryAbility.ets` no longer imports `@ohos.window` or has `onWindowStageCreate` with `windowStage.loadContent('pages/Index')`; window stays blank but `console.log` reaches hilog — enough to validate Phase 1's goal of "cross-compile → NAPI bind → TS main() executes". `module.json5` drops `pages: "$profile:main_pages"`, `main_pages.json` no longer emitted, `resources/base/profile/` no longer created. **B.6 (v0.5.129) — native-object pickup**: `compile/link.rs` walks `target/<perry-auto-*>/<triple>/release/build/*/out/` for loose `.o` files emitted by `cc-rs` build scripts (notably `libmimalloc-sys`, which produces a 362-KB `<hash>-static.o` containing 154 mi_* symbols). Rust's staticlib normally bundles these into `libperry_runtime.a`, but on macOS→OHOS cross-builds the `libmimalloc.a` wrapper comes out as a zero-member BSD-format archive (BSD ar's `__.SYMDEF SORTED` layout — macOS-host `ar` creates it, llvm-ar can't read it back) and rustc's "bundle native libs into staticlib" silently skips it. Without forwarding the loose `.o` files to the final link, `libentry.so` ends up with `mi_malloc_aligned` marked UND, and the OHOS dynamic linker rejects dlopen at `EntryAbility.onCreate` with "symbol not found." Walked-pickup is coarser than Rust's per-crate link-lib directive walking (picks up `.o` from any transitive C dep, not just mimalloc), but mimalloc is the only C dep in perry-runtime's closure today and unreferenced ones are dead-stripped via the existing `--gc-sections`. **B.4 (v0.5.127) — earlier audit fixes** mostly bundle into B.5/B.6 above (`-appCertFile` vs `-profileFile` distinction in the hap-sign CLI invocation, `developtools_hapsigner` README pointers in code comments). **Cherry-pick fold-in**: 3 cherry-picks across 3 commits required Cargo.toml + CLAUDE.md conflict resolution per commit (mechanical). compile.rs conflicts taken-as-ours each time and the meaningful new hunks (linker libs, native-object pickup) hand-applied to their current homes in `compile/link.rs` since main has refactored that code out of compile.rs. The harmonyos_hap.rs es2abc rewrite + EntryAbility.ets simplification auto-merged cleanly. Bumped 0.5.129 → **0.5.369** (above main's current 0.5.368 from PR #251).
