# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**NOTE**: Keep this file concise. Detailed changelogs live in CHANGELOG.md.

## Project Overview

Perry is a native TypeScript compiler written in Rust that compiles TypeScript source code directly to native executables. It uses SWC for TypeScript parsing and LLVM for code generation.

**Current Version:** 0.5.749


## TypeScript Parity Status

Tracked via the gap test suite (`test-files/test_gap_*.ts`, 28 tests). Compared byte-for-byte against `node --experimental-strip-types`. Run via `/tmp/run_gap_tests.sh` after `cargo build --release -p perry-runtime -p perry-stdlib -p perry`.

**Last full sweep:** run `./run_parity_tests.sh` for the current snapshot — the gap-suite top-line has shifted with recent landings; see open issues #447 (nested-async hang masking `test_async`), #448 (`*[Symbol.iterator]()` class hang), #449 (`new.target` → NaN), #450 (`defineProperty` accessor `this`), #451 (`test_gap_json_advanced` SIGSEGV), and #452 (`Array.fromAsync` / Proxy/Reflect / JSON ordering). Several of these are pre-existing bugs that the parity skip-list and gap-suite output truncation hid from earlier numbers.

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
cargo test --release --workspace \
  --exclude perry-ui-ios --exclude perry-ui-tvos --exclude perry-ui-watchos \
  --exclude perry-ui-visionos --exclude perry-ui-android --exclude perry-ui-windows \
  --exclude perry-ui-gtk4   # Run tests (exclude cross-host UI crates on macOS)
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

One-liners only — full detail in CHANGELOG.md.

- **v0.5.749** — Closes #617 (`await fetchWithAuth(url, auth)` / `await fetchPostWithAuth(...)` inline returned `undefined`; the explicit two-step `let p = fetchWithAuth(...); await p;` worked). Mechanizes the user's verified workaround at the HIR level: in `crates/perry-transform/src/async_to_generator.rs`, `hoist_awaits_avoiding_top_level` and `hoist_awaits_in_expr_full` now hoist `Expr::FetchGetWithAuth` / `Expr::FetchPostWithAuth` operands of `Expr::Await` into a fresh `let __await_fetch_N = ...` before the await — so the generator-state-machine's `Yield(value, done)` iter-result object always reads the promise back via `LocalGet`, never as an inline `FetchGetWithAuth` field initializer. Regression test in `test-files/test_issue_617_inline_await_fetch_with_auth.ts`.
- **v0.5.748** — Refs #420 / #618 followup: static field access via Any-typed local holding a class ref now works. Pre-fix `const X: any = A; X.kind` returned `undefined` because the codegen's PIC fast path treated INT32-tagged class refs (top16=0x7FFE) as non-pointer values and routed to the invalid arm. Drizzle's `is(value, type)` chain reads `cls.kind` through Any-typed locals; without this fix the entire entity-kind dispatch returns the wrong answer. Three coordinated layers: (1) **codegen PIC arm** in `crates/perry-codegen/src/expr.rs::Expr::PropertyGet` adds an `is_int32_class` check (top16 == 0x7FFE) that BEFORE the existing pointer-validity arm routes through `js_object_get_field_by_name_f64` with the unmasked `obj_bits` so the runtime can detect the INT32 tag; (2) **runtime helper** `js_class_register_static_field(class_id, name_ptr, name_len, value)` in `crates/perry-runtime/src/object.rs` populates `CLASS_DYNAMIC_PROPS` so static-field lookups via the runtime path find the value — codegen calls this from `Expr::StaticFieldSet` lowering for every initializer in addition to writing the per-field module global; (3) **synthetic `constructor` intercept** in `js_object_get_field_by_name` returns the class ref for an instance receiver (reads `(*obj).class_id` and returns the INT32-tagged ref if registered), so `instance.constructor` matches Node behavior. Repro: `class A { static kind = "A" } const X: any = A; X.kind` returns `"A"` matching Node (was `undefined`). The IC-miss-side parallel intercept was tried but reverted — small handle ids from native modules (axios / fastify / fetch handles) numerically collide with class_ids, and the runtime can't distinguish them after the codegen unbox; the codegen-side approach preserves the tag and is unambiguous. Sanity: 15 tests pass + hono entry.ts byte-identical (r1=200/r2=200/r3=404). New regression test test_static_field_via_local.ts.
- **v0.5.747** — perf(transform): interprocedural deforestation pass (`crates/perry-transform/src/deforest.rs`). Detects the **producer/consumer fusion shape** — a function that allocates a fresh array, fills it via `out.push(...)` (and possibly recursive self-calls whose results are spliced into `out` via the canonical `for(j) out.push(child[j])` consume loop), and returns the array — and rewrites the producer to take an accumulator out-parameter, the consume loops to fuse with the recursive call, and the top-level callers to pre-allocate the accumulator. **End-to-end on aya_koto's ABC451D bench** (Zenn article reader's "Perry is 6.5× slower than Bun" complaint, originally diagnosed in v0.5.736 as GC-throughput-bound on string[] allocation pressure with no single-commit fix): Perry drops from **3165 ms → 760 ms** (4.16× faster than baseline), within **1.75× of Bun** (411 ms) — was 7.3× slower pre-fix. The transform fires INSIDE the recursive `search()` call: each call's intermediate `child` array is eliminated (the inner recursion writes directly into the top-level accumulator, no per-call allocation, no copy loop), so a previously O(N) array-allocation chain collapses to O(1). **Three coordinated guards** keep the rewrite sound (initial implementation broke 4 of 8 affected tests until they landed): (a) skip exported functions — their callers may live in other modules this intra-module pass can't see; (b) skip producers whose body contains any closure expression — the safe-pattern analyzer doesn't walk closure bodies, so a `forEach((x) => out.push(x))` would slip past detection but break at substitution time; (c) skip producers whose calls appear in expression positions (`f(args).join(",")`, `someFn(f(args))`, `return f(args)`) — the rewritten producer returns `undefined`, so any caller using the return as a value would silently break. Detection pass: 3 phases — (1) per-function shape analysis, (2) producer-as-value detection (bail if `Expr::FuncRef(f)` ever appears in non-callee position, e.g. `const g = f; g(args)`), (3) call-site shape verification (bail if any call lives outside the supported `let X = f(args);` or bare `f(args);` shapes). Transformation: subst `LocalGet(out_local)` → `LocalGet(out_param)` everywhere including HIR variants with direct `LocalId` fields (`ArrayPush.array_id`, `ArrayPushSpread.array_id`, `ArrayPop`, `ArrayShift`, `ArrayUnshift.array_id` — not Expr children, missed by the generic walker). Runs BEFORE `inline_functions` so the inliner sees the rewritten signatures. Scope: intra-module only (cross-module deforestation needs whole-program analysis or a wrapper-shim layer that preserves the original signature for external callers — separate followup). 11 unit tests cover bucket math + gating state machine; 5 detection-edge tests cover async / multi-return / reassignment / out-passed-to-call / capture-into-closure rejection. Detection fires on 8 functions across the test-files corpus; all 8 affected tests + 12 sanity tests + 27/28 gap tests pass byte-identical to Node (the 1 failure, `test_gap_console_methods`, is a pre-existing timing-sensitive `console.time` divergence unrelated to deforestation). Stretch goal of 1.5 s on ABC451D not hit (Bun's V8 also gets escape analysis for the `const Y = a + b` intermediate string allocations, which Perry's intra-function escape analysis already handles for object literals but not strings yet — the Bun gap is now string-allocation-side, not array-allocation-side).
- **v0.5.746** — Refs #420 / #618 followup: `instance.constructor` now returns the class ref (was `undefined`). The keys_array lookup never finds `"constructor"` because the class isn't stored as a field on the instance — the chain fell through to `undefined`. Drizzle's `is(value, type)` chain uses `value.constructor[entityKind]` to identify class types; without `.constructor` working the entire entity-kind dispatch returns false. Fix: added a `"constructor"` key intercept in `js_object_get_field_by_name` (in `crates/perry-runtime/src/object.rs`) that fires AFTER the type-tag bailouts but BEFORE the keys_array lookup. Reads `(*obj).class_id`; if non-zero AND registered (consults v0.5.745's `is_class_id_registered` helper), returns the INT32-tagged class ref (`0x7FFE_0000_0000_0000 | class_id`). Pure object literals (class_id=0) and unregistered class refs still return undefined — matches Node behavior where bare object literals don't have a custom `.constructor`. Repro: `class SQL {}; const s = new SQL(); typeof s.constructor` returns `"function"` and `s.constructor === SQL` is true (was: `"undefined"` and false). Drizzle's `is()` chain now correctly returns `true` for `is(new Aliased(), Aliased)` and `false` for `is(new Aliased(), SQL)`. Sanity: 8 tests pass. New regression test test_instance_constructor.ts.
- **v0.5.745** — Refs #420 / #618 followup: `typeof` of a class ref obtained via `PropertyGet` (or `LocalGet` after dynamic assignment) now returns `"function"` instead of `"number"`. Class refs are NaN-boxed as `INT32_TAG | class_id` (top16=0x7FFE); `js_value_typeof`'s is_int32 branch fell through to `"number"` for them. The previous v0.5.743 fix only worked for direct ClassRef expressions (special-cased at codegen TypeOf lowering); chained access like `(SQL as any).Aliased` lowered via the runtime path and missed. Three layers: (1) new `REGISTERED_CLASS_IDS: RwLock<Option<HashSet<u32>>>` thread-local registry in `crates/perry-runtime/src/object.rs` with public `is_class_id_registered(class_id) -> bool` helper; (2) new `js_register_class_id(class_id: i32)` runtime FFI declared in `crates/perry-codegen/src/runtime_decls.rs` and called once per class at module init from `emit_string_pool` in `crates/perry-codegen/src/codegen.rs` (collects ids from `hir.classes` + `class_ids` map, dedupes, registers all); (3) `js_value_typeof` is_int32 arm in `crates/perry-runtime/src/builtins.rs` consults the registry — registered class id → `"function"`, otherwise → `"number"`. The CLASS_VTABLE_REGISTRY fallback inside is_class_id_registered (kept for safety) covers any class that registers methods even if the explicit id-register hasn't fired. Repro from drizzle's load-bearing site: `class SQL {}; ((S:any)=>{class Aliased{}; S.Aliased=Aliased;})(SQL); typeof (SQL as any).Aliased` returns `"function"` matching Node (was `"number"`). Sanity: 7 tests pass (test_inheritance / test_simple_class / test_break_continue / test_edge_promises / test_gap_async_advanced / test_gap_typeof_instanceof / test_gap_class_advanced). New regression test test_typeof_class_via_get.ts. **Out of scope (next iteration)**: `instanceof` for INT32-tagged class refs still returns truthy for non-matching classes — drizzle's `is(value, type)` chain has its own remaining blocker.
- **v0.5.744** — Refs #420 / #618: Symbol-keyed static class fields now readable via `cls[symbol]` and `symbol in cls`. Pre-fix `class A { static [KIND] = "A-kind" }` registered the value in `CLASS_STATIC_SYMBOLS` at module init via `js_class_register_static_symbol`, but: (1) `js_object_get_symbol_property` only consulted the per-instance `SYMBOL_PROPERTIES` map and returned undefined for class refs; (2) `js_object_has_property` (the `in` operator) bailed early for non-pointer receivers, so `KIND in A` returned false. Both runtime fns now detect INT32-tagged class refs (`top16 == 0x7FFE`) and route through `class_static_symbol_lookup`. Drizzle's `cls[entityKind]` and `entityKind in cls` chain inside `is(value, type)` now correctly identifies class types. Sanity: 9 tests + hono entry.ts byte-identical (r1=200, r2=200, r3=404). New regression test test_class_static_symbol.ts. **Out of scope (separate followup)**: drizzle's `db.insert(...).run()` still fails — the next blocker is in the `is(value, type)` chain, where `value instanceof type` returns truthy for non-matching classes (Perry's instanceof for INT32-tagged class refs has its own dispatch quirks). Filing as next iteration.
- **v0.5.743** — Refs #618 / #420 (drizzle): static properties added to imported classes via the IIFE pattern `((SQL2) => { SQL2.Aliased = Aliased; })(SQL)` now persist.
- **v0.5.742** — Refs #420 / #488 (drizzle): tightened the `.join()` array-fold gates in `crates/perry-hir/src/lower/expr_call.rs` so user objects with their own `.join` static method are not misrouted to `Array.prototype.join`.
- **v0.5.741** — Refs #420 / #631 followup: 4-level inheritance with intermediate-class field initializers depending on parent body state.
- **v0.5.740** — Refs #637 final followup: forEach-captured-array IndexSet with Any-typed string key now works.
- **v0.5.739** — Refs #637 followup: extends `js_array_set_string_key` to also handle Object/Closure receivers (gc_type detection at entry — falls through to `js_object_set_field_by_name` for non-array). Routes the polymorphic IndexSet's str_set arm at `crates/perry-codegen/src/expr.rs::Expr::IndexSet` through this helper instead of the unconditional object-setter call.
- **v0.5.738** — JSON.stringify cosmetic followup to v0.5.737: sparse-array gap-fill in `js_array_set_f64_extend` now writes `TAG_HOLE` instead of raw `0.0`.
- **v0.5.737** — Closes #637 / #603 (hono r2 param routes): three more coordinated fixes that get param-route extraction working end-to-end.
- **v0.5.736** — BigInt small-int fast path + slow-path early-exit (`crates/perry-runtime/src/bigint.rs`).
- **v0.5.735** — Closes #638 (hono r2 RegExpRouter trie regex corruption): `regexp.replace(/regex/g, callback)` where receiver was Any-typed (e.g. result of `this.#privateField.method()`) and callback was a closure literal returning a string literal had the closure coerced to "[object Object]" via `js_native_call_method`'s string-replacement fallback path.
- **v0.5.734** — Refs #637 (hono r2 RegExpRouter): two more coordinated fixes that get the param-route detection unblocked.
- **v0.5.733** — Refs #637 (hono r2 RegExpRouter): two coordinated fixes that unblock `regex.source` / `.flags` survival across function boundaries and the property-IC false-hit on non-Object headers sharing GC_TYPE_OBJECT.
- **v0.5.732** — Closes #609 (`Buffer.toString('utf8')` on bytes that aren't valid UTF-8 SIGSEGV'd the runtime).
- **v0.5.730** — Closes #636 (namespace member call dispatches receiver as TAG_TRUE → `(boolean).fn is not a function`).
- **v0.5.729** — Closes #610 (`ForEach(state<number>, render)` didn't re-render when the bound state changed via `.set()`). #599 (state.text) shipped in v0.5.706 but the `ForEach` half from the same reproducer wasn't covered. Mirrors the v0.5.706 architecture (text_registry pattern).
- **v0.5.728** — Closes #633 partial (`.then()` on async-arrow's call result silently dropped its callback). `is_promise_expr` in `crates/perry-codegen/src/type_analysis.rs` only recognized `Expr::FuncRef` calls (top-level async function decls registered in `cross_module.local_async_funcs`) as Promise-returning — it did NOT recognize `Expr::LocalGet(let_id)` calls where the…
- **v0.5.727** — Refs #611 (Effect SIGSEGV bisected): `globalThis[<computedKey>] = value` and `(globalThis as any)[id] ??= new Map()` patterns wrote to a 0-pointer sentinel and read back undefined.
- **v0.5.726** — Closes #632 (hono r1 regressed from 200 to 404 after v0.5.722's dynamic-dispatch override probe).
- **v0.5.725** — Closes #627 (`axios.get(url).then(r => r.data.ok)` — `r.data` should auto-parse JSON when Content-Type is `application/json`, matching npm axios).
- **v0.5.724** — Closes #630 (`Buffer.allocUnsafe(size)` and `Buffer.byteLength(s, encoding)` failed at codegen Phase 2 — "expression BufferAllocUnsafe not yet supported").
- **v0.5.723** — Closes #629 (`import * as fsp from "node:fs/promises"` resolved to TAG_TRUE → `typeof fsp === "boolean"` → confusing `(boolean).readFile is not a function` errors). Two-part fix.
- **v0.5.722** — Refs #603 / #620 followup: `this.method = X` override now also fires on the dynamic-dispatch path.
- **v0.5.721** — Closes #628 (`const m = obj.method; m(...)` returned undefined for class-method bound closures).
- **v0.5.720** — Closes #618 (function-static properties via IIFE pattern — `((sql2) => { sql2.identifier = ...; })(sql)` — drizzle-orm's `sql` tag and TypeScript's namespace-on-function emit).
- **v0.5.719** — Closes #616 (TypeError uncaught-throw output diverges from Node — `Uncaught exception:` prefix line + duplicate stack header).
- **v0.5.718** — Closes #624 (compilePackages: synthetic node_modules pkg with `module.exports = { fn }` failed at link, then on link-fix surfaced an unrelated runtime call-returns-undefined regression in IIFE-wrapped CJS modules).
- **v0.5.731** — Refs #633 (hono r2 box_get warnings): closes the closure-capture-from-hoisted-async-fn-decl bug surfaced in the synthetic compose pattern.
- **v0.5.717** — Closes #623 (`typeof process` / `typeof console` / `typeof globalThis` returned `"number"` instead of `"object"`). `import process from "node:process"` lowers to HIR `NativeModuleRef("process")`, which the codegen represents as a `0.0` stub double (`crates/perry-codegen/src/expr.rs:8927`); `js_value_typeof` correctly identifies that as `"number"`.
- **v0.5.716** — Refs #603 (hono routes-with-params): closes the three bisected blockers (#619 / #620 / #621).
- **v0.5.715** — **Closes #604 #625 #626 #627** (axios.get + node:http hangs after `server.close()` — full architectural fix).
- **v0.5.714** — v0.5.713 broke `cargo build --release -p perry` because `crates/perry/src/commands/compile/optimized_libs.rs:170` calls `super::well_known::bundled_staticlib_path_for_target(workspace_root, binding, rust_target_triple(target))` (3-arg, target-aware) but `crates/perry/src/commands/compile/well_known.rs:74` only had `bundled_staticlib_path` (2-arg, host-only).
- **v0.5.713** — Closes the three baseline parity gaps (#613 / #614 / #615) flagged in CLAUDE.md and tracked by tier 2 of the release-sweep harness.
- **v0.5.712** — Three leaked `eprintln!` debug lines in `crates/perry-ext-http-server/src/{server,http2_server,https_server}.rs` (line 219 / 254 / 261 — `[node:http] listening on http://...`, `[node:http2] listening on https://...`, `[node:https] listening on https://...`) fired on every `server.listen()` call.
- **v0.5.711** — Closes #542/#543 followup: `m.keys()` / `m.values()` / `m.entries()` on a `Map<K, V> | undefined` parameter narrowed via `if (!m) return;` returned the array-fold shape (`[0..N-1]` for keys, garbage for values) because `crates/perry-hir/src/lower/expr_call.rs:3319` only matched bare `Type::Generic { base: "Map" }` for the receiver-type predicate.
- **v0.5.710** — Parity regression in v0.5.707: 39 tests failing (82.7% rate) vs the v0.5.696 baseline of 6 (97.3%).
- **v0.5.709** — `cargo test --workspace` was still red after v0.5.708 due to the parallel `manifest_consistency` invariant: `every_dispatch_entry_has_manifest_counterpart` panicked because `NATIVE_MODULE_TABLE` had `ioredis::connect` and `ioredis::disconnect` rows (added in v0.5.707's #605 fix when normalizing the `redis` npm alias) but `API_MANIFEST` had no matching…
- **v0.5.708** — `cargo test --workspace` regression from v0.5.707: the `imported_func_has_rest` field added to `CompileOptions` for #608's cross-module rest-param plumbing was missed in the `empty_opts()` test helper at `crates/perry/src/commands/compile/object_cache.rs:514`. `cargo test -p perry` failed with E0063 missing-field; tier 1 of the release-sweep harness…
- **v0.5.707** — Multi-issue release-sweep cycle (#601–#608). **#601** (perry-ext-fetch tests broken at workspace `cargo test`): `crates/perry-ext-fetch/src/lib.rs:1140/1179/1196` — three test-code type mismatches against the post-Phase-1 NaN-boxed FFI surface (integer literal where f64 expected, missing 4th `headers_handle: f64` arg on `js_request_new`, `resp as i64` cast…
- **v0.5.706** — **Closes #599** (`state.text()` widget never updated on macOS after `state.set("new value")`).
- **v0.5.705** — **Closes #600** (LLVM Phase-2 codegen regression vs v0.5.491: `Expr::FetchGetWithAuth`, `Expr::FetchPostWithAuth`, `Expr::DateToLocaleString` had no LLVM arms — fell through to the catch-all "expression … not yet supported" error).
- **v0.5.704** — **Closes #598** (`axios.post(url, {email, password})` sent the raw bytes of Perry's `ObjectHeader` struct followed by the first character of the stringified field — server received `^@^B^@^@H...` instead of `{"email":"demo@example.com","password":"hunter2"}`).
- **v0.5.703** — **Closes #597** (drizzle #488 blocker: `for (const [i,v] of arr.entries())` silently iterates 0 times when `arr` is statically typed `any`).
- **v0.5.702** — **Closes #596** (sync throw inside async fn body — `obj.status` where `obj` is undefined, post-await — escaped the user's `try { await x; ... } catch { … }` and exited the program).
- **v0.5.701** — **Closes #594 #595** (`async function callIt() { try { ... await ... } catch {} ; if (cond) return X; }` returned `X` directly to the iterator caller, surfacing as `TypeError: Cannot read properties of undefined (reading 'done')` on `__step_r.done`).
- **v0.5.700** — **Closes #593** (`const off = ev.on(() => { off(); })` inside a function: closure body's `off()` no-ops because it captured the let's pre-init slot value).
- **v0.5.699** — **Closes #591** (`argon2.hash(password)` on perry-ext-argon2 reported "Invalid password" because the password StringHeader pointer never reached the FFI).
- **v0.5.698** — **Closes #592** (`String.prototype.toLocaleLowerCase` / `toLocaleUpperCase` were silently routed through the unknown-method catch-all in `crates/perry-codegen/src/lower_string_method.rs:675-709`, which emits `js_throw_type_error_not_a_function("string", "<prop>")` + `unreachable`.
- **v0.5.697** — **Closes #590** (`Object.assign(target, src)` was orphaning Symbol-keyed properties on `target` and breaking `result === target` identity / `target instanceof Class`).
- **v0.5.696** — **Closes #589 (runtime path)**: builds on v0.5.694's link-gap fix to make the issue's full acceptance criterion pass — `compilePackages: ["hono"]` + `import { createServer } from "node:http"` + `app.fetch(new Request(...))` + `fetchRes.headers.forEach((v,k) => res.setHeader(k,v))` + `res.end(await fetchRes.text())` now compiles, runs, and `curl…`
- **v0.5.695** — **Closes #588** (closure body `() => new <ImportedClass>(...)` inside a compilePackages module: link-fail or silent class-id-collapse — `_factory` reads back as `1.26e-321` regardless of what's assigned).
- **v0.5.694** — **Closes #589** (link gap: `import { Hono } from "hono"` + `import { createServer } from "node:http"` failed at link with `Undefined symbols: _js_headers_new / _js_response_new / _js_request_new`).
- **v0.5.693** — **Refs #551** — documentation only. Lists `@bradenmacdonald/s3-lite-client` as a known-working npm package alongside `hono` in `docs/src/packages/porting.md`, and adds a new "AWS S3 / S3-Compatible Object Storage" section to `docs/src/stdlib/http.md` (between WebSocket and Next Steps).
- **v0.5.692** — **Closes #585 #586** (drizzle-orm #488 query-side blockers).
- **v0.5.691** — **Closes #577** (Node-compat HTTP server modules — `node:http` + `node:https` + `node:http2` + WebSocket upgrade integration).
- **v0.5.684** — **Closes #583** (deep links: Universal Links / App Links / URL schemes).
- **v0.5.690** — **Closes #584** (`new TextEncoder().encode(s)` Uint8Array had wrong backing bytes — every byte read returned the IEEE-754 representation of the byte value, not the byte itself).
- **v0.5.677** — **Closes #582** (network reachability — online/offline + change events). Already shipped via the v0.5.676 omnibus + the follow-up commit `627cc9f9` that wired the missing dispatch / manifest / collect_modules / .d.ts surfaces and the GTK4 / visionOS / watchOS / Windows stubs.
- **v0.5.676** — **Refs #577** (HIR/codegen integration: idiomatic `import { createServer } from "node:http"` + `server.listen(...)` + handler closure with `req.method()` / `res.end(...)` now compiles + runs end-to-end).
- **v0.5.675** — **Closes #536** (`@perryts/mysql` hangs after `pool.end()` because `try { return await ... } finally { Y }` skipped Y entirely).
- **v0.5.674** — **Refs #552** (mobile stdlib gaps for production apps).
- **v0.5.668** — **Closes #578 #579** (s3-lite-client SigV4 byte-handling blockers from #551).
- **v0.5.665** — **Closes #575 #576** (s3-lite-client SigV4 blockers from #551).
- **v0.5.664** — **Closes #572 #573 #574** (s3-lite-client compile blockers from #551).
- **v0.5.663** — **Closes #569** (function-scoped `function` declarations not hoisted): inner `function helper(...) {...}` declarations inside a function body are now lowered with proper ECMAScript hoisting + initialization.
- **v0.5.662** — **Closes #535** (state→UI binding propagation outside `--target harmonyos`).
- **v0.5.661** — **Closes #567** (`String.prototype.split(separator, limit)`): pre-fix, the codegen path in `crates/perry-codegen/src/lower_string_method.rs` rejected the 2-arg form with `String.split expects 1 arg (delimiter), got 2` at compile time.
- **v0.5.660** — **Closes #486 (logger middleware)** — two coordinated fixes that together unblock the full hono `app.use('*', logger()) + app.get('/', c => c.json({...}))` chain end-to-end.
- **v0.5.658** — **Refs #486** (try/catch around self-recursive closure body breaks self-reference): `let dispatch = (i) => { try { dispatch(i+1); } catch (e) {} }` (and the equivalent `async function dispatch(i) { try { await dispatch(i+1); } catch (e) {} }` declared inside an async closure body) had its self-reference invisible to the boxing analysis.
- **v0.5.657** — Closes #494: native-libraries authoring guide now ships a real, compileable Rust fixture crate with drift-protection unit tests.
- **v0.5.655** — Closes #562: user classes can extend `WritableStream`/`ReadableStream`/`TransformStream` end-to-end (s3-lite-client `ObjectUploader` shape).
- **v0.5.654** — Closes #561: Web Crypto `crypto.subtle.{digest,importKey,sign,verify}` (HMAC + SHA-1/256/384/512) for AWS SigV4 / JWT signing chains.
- **v0.5.653** — Refs #486: generator state-machine pass now scans `catch(e){}` param so async-recursive try/catch with unused binding no longer clobbers `__gen_state`.
- **v0.5.652** — Refs #536: `[].pop()`/`[].shift()` return `undefined` (not `NaN`); perry-ext-net `active_handles` wired into stdlib event-loop gate.
- **v0.5.650** — Docs sweep — backfilled pages for #538/#552/#553/#532/#517/#473/#458/#491 + stdlib `other.md` cleanup.
- **v0.5.649** — Refs #486: `new RegExp(varname)` with non-literal pattern arg now lowers via `js_regexp_new` (was placeholder, broke hono wildcard middleware).
- **v0.5.648** — Refs #538 followup: real BGTaskScheduler / NSBackgroundActivityScheduler / WKApplication impls on tvOS / visionOS / watchOS / macOS; GTK4 + Windows kept as stubs.
- **v0.5.644** — Closes #538: new `perry/background` module — iOS BGTaskScheduler + Android WorkManager bindings (`registerTask`/`schedule`/`cancel`).
- **v0.5.643** — Refs #486: new `js_dynamic_string_or_number_add` runtime helper for type-uncertain `+` operands (string concat through Any-typed values).
- **v0.5.642** — Closes #544: per-module `.o` cache key mixes in a hash of the running perry binary, so HIR/codegen pass changes invalidate stale objects.
- **v0.5.641** — Refs #486: cross-module class-expression self-binding alias (`var X = class _X { ... new _X() }`) — the npm-dist convention now resolves correctly.
- **v0.5.640** — `.ts`/`.tsx` packages from `node_modules` now classified `NativeCompiled` at the import site even without a `perry.compilePackages` entry.
- **v0.5.639** — Closes #553: four production-mobile widgets — `BottomNavigation`, `ImageGallery`, pull-to-refresh, infinite-scroll (`onScrollEnd`).
- **v0.5.638** — Refs #486: cross-module method inliner now excludes methods whose body references a local non-exported class (preserves class metadata).
- **v0.5.637** — Refs #517 followup: `MapView` un-stubbed on tvOS / GTK4 (libshumate) / Android (Google Maps) / watchOS (SwiftUI Map); Windows still pending (#559).
- **v0.5.636** — Closes #532 + #555 (PR `ui-loop-issues`): 11 native UI widgets across macOS/GTK4/Windows/iOS/visionOS — `widgetSetRichTooltip`, `Combobox`, `TreeView`, `Calendar`, `Chart`, `commandPalette*`, `MapView`, `PdfView`, `RichTextEditor`, `Toast` watchOS, data-table sort+filter+multi-select. Plus `tableGetFilterText` FFI ABI fix (i64 vs f64 register).
- **v0.5.635** — Closes #557: zero-arg `console.log()`/`info`/`warn`/`error`/`debug` now emit a newline (was silent no-op).
- **v0.5.634** — Closes #540: `[...map]` spread now yields `[key, value]` pair arrays via Map-detection arm in `js_array_concat` (was garbage).
- **v0.5.633** — Closes #554 followup: nested `{ key: [a, b] }` for-of destructure now also works inside function bodies (`lower_decl.rs` parallel path).
- **v0.5.632** — Refs #552: Android geolocation + photo-picker real impl (LocationManager + Photo Picker) replacing v0.5.631 stub.
- **v0.5.631** — Refs #552: `geolocationGetCurrent`/`geolocationWatch`/`imagePickerPick` on `perry/system` — macOS + iOS real; Android stubbed (replaced in v0.5.632).
- **v0.5.630** — PR #534 (dazhe): `codegen-wasm` `main()` with no return now emits valid Wasm; `has_return` recurses into DoWhile / For.init / Labeled.
- **v0.5.629** — Closes #554: nested destructuring in for-of `const` binding (`for (const { entity, components: [a, b] } of ...)`) now binds leaves correctly.
- **v0.5.628** — Closes #549: Set/Map object key collisions — `is_string_like` validates GC type tag at the pointee, so `set.add({})` doesn't collapse same-shape objects.
- **v0.5.627** — Closes #546/#547/#548: array `.some`/`.every`/`.find`/`.findIndex`/`.findLast(Index)` arms added for any-typed receivers + JSON.stringify defensive UTF-8 fallback.
- **v0.5.626** — Closes #420: drizzle-orm compiles + runs end-to-end via `compilePackages` — `pgTable` + `eq`/`gt`/`and` byte-for-byte matching Node.
- **v0.5.625** — Refs #420: derived-class field initializers now run AFTER parent ctor body per ECMAScript spec (new `FieldInitMode::AncestorsOnly`/`SelfOnly` enum).
- **v0.5.624** — Closes #541/#542/#543: codehz/ecs Map iteration through `Map<K,V> | undefined` parameters — `is_iterable_map` now accepts Union types; array-method fold gated on static type.
- **v0.5.623** — Refs #535 Layer 2: `NavStack(state, routes)` lowering for native multi-screen on macOS/iOS/Android/GTK4/Windows.
- **v0.5.622** — Refs #420: classes carrying `static [Symbol.for(...)] = "..."` fields — `Object.prototype.hasOwnProperty.call(C, sym)` now consults a side-table.
- **v0.5.621** — Refs #535 Layer 1: target-agnostic `state<T>(initial)` desugar for non-HarmonyOS — runtime `STATE_VALUES` registry + setText pipeline.
- **v0.5.620** — Refs #486: hono v0.5.614 forward-port — declared-but-uninitialized class fields default to `undefined`; `response.headers` arm; cross-module class getters registered.
- **v0.5.619** — Closes #533: for-of through `m.get(k)!` on Map-of-Maps — `infer_call_return_type` learned Map.get/Set.has, `new Map([entries])` infers K/V from literal entries.
- **v0.5.618** — Refs #420: `Object.prototype.hasOwnProperty.call(o, k)` rewritten to `js_object_has_own`; Symbol keys consult `SYMBOL_PROPERTIES` side table.
- **v0.5.617** — Refs #486: relative imports inside compile-package roots (`file:./` deps + symlinked node_modules) now resolve to `NativeCompiled` correctly.
- **v0.5.616** — Refs #420: 7 cross-module 3-level-inheritance fixes for drizzle pgTable — origin-path imported_vars lookup, ctor-less ancestor super-chain walk, vtable parent-chain dispatch, etc.
- **v0.5.615** — Refs #420: cross-module static class fields — source-side globals flip to external linkage; `ImportedClass.static_field_names` populated; `Expr::PropertyGet` routes to global load.
- **v0.5.614** — Refs #420: computed-key class fields (`[Sym]= …`) reach codegen via `ClassField.key_expr`; static field inits emit `Stmt::StaticFieldSet` in source order so they see top-level consts.
- **v0.5.613** — Closes #421: hono compiles + runs end-to-end via `compilePackages` — handle-shaped pointers skipped in `format_jsvalue`/`js_instanceof`; method-dispatch arms for Response/Blob/Headers handles.
- **v0.5.612** — Refs #421: any-typed string `indexOf`/`lastIndexOf` return f64 (not NaN-tagged INT32); `CLOSURE_ARITY_REGISTRY` pads under-arity calls with `TAG_UNDEFINED`.
- **v0.5.611** — Closes #531: generator-transform `compute_max_func_id`/`_local_id` walkers now scan `Expr::ArrayPush`/`ArrayPushSpread` args (closures inside ops.push were invisible).
- **v0.5.610** — Refs #421 (followup to #608): `recv.method(...args)` on any-typed receivers via new `js_native_call_method_apply`; `replace`/`replaceAll` arms on any-typed string dispatch tower.
- **v0.5.609** — Closes #528 (followup to #515/#518): chained-call array-method fold leaked through the `recv_is_class` `_ => false` catch-all — `this.col().find({})` got rewritten as `Expr::ArrayFind(this.col(), {})` and `js_array_find` read garbage from the user object's header. New `Call(inner)` arm in `recv_is_class` only allows the fold when the inner call's method is a known array-producing builtin; otherwise bails to runtime dispatch.
- **v0.5.608** — Refs #421: Phase 1 of handle-NaN-boxing unification — Web Fetch (Request/Response/Headers/Blob) handles now NaN-boxed at the FFI boundary; 25 fetch FFI fns migrated, 4 untyped property dispatch helpers added. Unblocks hono `request.url` in untyped position.
- **v0.5.607** — Closes #529 (followup to #515): `obj["method"](args)` / `obj["prop"]` on a class instance returned `undefined` — fold computed `MemberProp` with non-numeric string key to `Expr::PropertyGet` so dispatch hits the vtable; mirror fold in assignment form.
- **v0.5.606** — Closes #526: escape JS reserved words used as method names in generated `.d.ts` (axios.delete now emits `function _delete; export { _delete as delete }`).
- **v0.5.605** — Closes #515 (round 2): tighten `with`-method fold gates for object-literal / chained-method-call / typed-array receivers; add layered codegen + runtime fallbacks.
- **v0.5.604** — Closes #525: unify R005/#463 unimplemented-API message format for the 2-deep native-module call form (`m.method()`).
- **v0.5.603** — Refs #519/#421: register Request/Response/Headers typed-parameter annotations as native instances at HIR lowering so codegen's static dispatch fires.
- **v0.5.602** — Closes #509: parallel-codegen race producing colliding `.o` files — temp path nonce now uses an atomic counter alongside pid+nanos.
- **v0.5.601** — Closes #519: bind `this` for `instance.fn(args)` style cross-instance method calls via thread-local `IMPLICIT_THIS`; fill string-receiver `match`/`matchAll`/`search` arms.
- **v0.5.600** — Closes #461: pre-register `const f = (...) => …` arrow declarations as locals before lowering their initializer, so nested closures referencing `f` find it via `lookup_local`.
- **v0.5.600** — Closes #513: API manifest now enumerates every `NATIVE_MODULES` entry + every well-known binding (~250 entries backfilled) so the unimplemented-API gate flips strict for every supported module.
- **v0.5.599** — Closes #518: async class method's `await m.toArray()` returned `[]` because scalar-replacement of non-escaping object literals dropped the receiver-as-`this` for `js_native_call_method`.
- **v0.5.598** — Closes #512: API manifest now carries real param/return type signatures (`bcrypt.hash(password: string, …)`) instead of `(...args: any[]): any` fallback.
- **v0.5.597** — Closes #511 (followup to #462): `x.foo()` on undefined/null now throws `TypeError` and exits 1 (was silent no-op exit 0).
- **v0.5.596** — Refs #519: vtable `this` now NaN-boxed with POINTER_TAG before f64 cast so dispatched method bodies see a real instance pointer.
- **v0.5.595** — Closes #515: a class method named `with` with ≥2 params was getting folded into `Expr::ArrayWith` — gate the fold catch-all on `!recv_is_class`.
- **v0.5.594** — Closes #514: `s[0]` / `s.at(-1)` on a `(s: any)` parameter holding a string now correctly indexes via `js_dyn_index_get`; runtime tag-aware dispatch added for any-typed string methods.
- **v0.5.593** — Closes #510 (followup to #462): calling a method on a primitive whose name doesn't resolve now throws `TypeError: <kind>.<prop> is not a function` (was silent no-op).
- **v0.5.592** — Closes #471: `Record<number, T>` writes via `obj[i] = v` corrupted the heap — replace inline `obj+8+idx*8` store with type-dispatching `js_object_set_index_polymorphic`.
- **v0.5.591** — Refs #421/#420: cross-module `Call { callee: ExternFuncRef }` for an imported VARIABLE binding now fetches the closure value via the zero-arg getter and dispatches through `js_closure_callN`.
- **v0.5.590** — Closes #490: cross-platform system-tray icon API (`trayCreate`/`traySetIcon`/`traySetTooltip`/`trayAttachMenu`/`trayOnClick`/`trayDestroy`) on macOS / Windows / Linux + iOS/tvOS/visionOS/watchOS/Android stubs.
- **v0.5.589** — Closes #493: closure rest-param bundling now happens at runtime via a `func_ptr → fixed_arity` registry, fixing dynamic-dispatch call sites where codegen can't statically bundle.
- **v0.5.588** — Closes #507: perry-ext-net `Handle::current()` panic — auto-optimize now rebuilds tokio-using ext bindings into the same target dir as perry-stdlib so cargo unifies tokio compilations.
- **v0.5.587** — Closes #458/#491: WAV recording (`audioSetOutputFilename`/`audioStartRecording`/`audioStopRecording`) implemented on every UI backend (macOS/iOS/tvOS/visionOS/watchOS/Android/Windows; Linux+Web shipped in #458).
- **v0.5.586** — Closes #485: cross-module subclass field initializers + arrow class fields installed in parent ctor body now correctly inherit (unblocks hono `app.fetch` chain).
- **v0.5.585** — Refs #139: floating-point fast-math (`reassoc + contract` FMF) is now opt-in via `--fast-math` / `PERRY_FAST_MATH=1` / `package.json`. Default OFF means bit-exact f64 with Node on most code.
- **v0.5.584** — Closes #484: rest-parameter bundling for class method dispatch — parallel `method_has_rest` map populated alongside `method_param_counts`, threaded into the call site.

Older entries → CHANGELOG.md.
