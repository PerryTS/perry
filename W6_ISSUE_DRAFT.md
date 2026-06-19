# Lazy default-import of a cjs-wrapped module binds to a named class export instead of `export default` (scale-emergent)

## Summary
In a large compiled bundle, a `require()` **inside a function** of a CommonJS-wrapped
module binds the import local to the module's **named class export** instead of its
**`export default`** (`module.exports`) object. The metadata is correct; only the
final codegen symbol binding is wrong, and only at giant-module scale.

## Concrete instance (Next.js app-router render → HTTP 500)
- Module: `next/dist/server/lib/incremental-cache/shared-cache-controls.external.js`
- Source exports (CJS): `Object.defineProperty(exports, "SharedCacheControls", { get })` + a top-level `class SharedCacheControls`.
- `cjs_wrap` output (correct): hoists the class, emits both `export default _cjs;` and `export { SharedCacheControls };`.
- `PERRY_DUMP_EXPORTS` (recorded metadata, correct):
  - `Named { local: "default", exported: "default" }`  (→ `_cjs`, the exports object)
  - `Named { local: "SharedCacheControls", exported: "SharedCacheControls" }`  (the class)
- Importer `app-page-turbo.runtime.prod.js` does `const uw = require(".../shared-cache-controls.external.js")` **inside `IncrementalCache.getIncrementalCache`**, recorded as:
  - `Default { local: "_lazyreq_26" }`, `is_adopted_require = true`  (a lazy default import)
- Runtime: `typeof uw === "function"` and `uw.SharedCacheControls === undefined` → `new uw.SharedCacheControls(...)` throws **`TypeError: undefined is not a constructor`** in `IncrementalCache`'s constructor → app-router render returns HTTP 500.

So the lazy default import `_lazyreq_26` binds to the **class** symbol instead of the
`"default"` (`_cjs`) symbol.

## What's ruled out
- `cjs_wrap` output — correct (`export default _cjs` present).
- Runtime `module.exports` — correct (`typeof module.exports === "object"`, `.SharedCacheControls` a function, `exports === module.exports`).
- `reachability.rs` — tree-shaking only; `shared-cache-controls` is a non-barrel → module-granularity (whole module kept).
- Default-export-name collision — `__default` symbols are per-origin (`perry_fn_<origin>__default`).

## Not minimally reproducible (~13 shapes all bind correctly)
relative `.js`; node_modules pkg; `.external.js` suffix; `compilePackages` NativeCompiled;
full-subpath require; within-package sibling require; circular require;
`exports.X = X`; `module.exports.X`; getter + static-field exact class shape;
dual-importer (named + namespace); **lazy require inside a function**;
**multi-module (5) lazy default imports**. Every one returns `_cjs`/binds correctly.
The defect appears only inside the real ~600KB `app-page-turbo` module.

## Likely area
Codegen default-import → export-symbol resolution (`import_function_prefixes` /
`perry_fn_<mod>__default`). Hypotheses: the `__default` symbol for an
`export default <identifier-expr>` (the IIFE result `_cjs`) is not emitted / not
reachable at scale, so the default import falls back to the module's other (named
class) export; or a scale-only symbol-resolution path differs.

## Repro env
`/tmp/perry-nextjs-demo` (Next 16 standalone, `output: 'standalone'`), compiled with
`PERRY_LL_O0_THRESHOLD_BYTES=536870912 PERRY_ALLOW_PERRY_FEATURES=1 PERRY_ALLOW_EVAL=1 PERRY_ALLOW_UNIMPLEMENTED=1`.
Diagnostic: `PERRY_DUMP_EXPORTS` dump added to `bootstrap.rs enforce_package_default_exports`.

## Context
This is the 6th wall in the Next.js app-router bring-up; walls 1–5 fixed on
`feat/nextjs-wall-46` (incl. `9970fbbe7` 0-arg class-object resolve, `af8c832b0`
readFileSync ENOENT, `6c41417ff` anon-class-expression capture). With W6 fixed the
render should advance past `IncrementalCache` construction.

---
## DEEP UPDATE (corrected root via runtime probes)
Earlier "binds to the class" was WRONG. Confirmed via ~10 probe cycles:
- Importer: `_lazyreq_26` is in `imported_vars`, NOT in `class_ids` → correctly reaches the getter path (dyn_extern_i18n.rs:594/625), calls `perry_fn_<mod>__default`.
- Exporter: that getter IS emitted (`emit_getter=true`, `is_function_alias=false`), loads `@perry_global_<mod>__55`.
- HIR: `export default _cjs` → `LocalGet(0)`; local 0 = `_cjs`, init = `Call` (the IIFE call result — correct).
- Module scope: at shared-cache-controls's OWN scope, `module.exports` (= `_cjs`) is `typeof object` (PERRY_SCC probe).
- Cross-module runtime (W6X at `new uw.SharedCacheControls`): `uw` = an UNNAMED CLOSURE (`typeof function`, `name===""`, no keys) — NOT the class, NOT the object.

So: `perry_global_<mod>__55` (the `"default"` global) holds a **closure** at runtime, even though `_cjs` is the exports **object** at its own scope. The cross-module `"default"` transfer (the module-init `perry_global__55 = LocalGet(0)` assignment, or the IIFE-result local read) **mistypes the object as a closure**, ONLY at giant-bundle scale (~14 minimal repros — incl. lazy-require, deferred, -O3 auto-optimize, exact class shape — all transfer the object correctly). Not the I64/F64 module_var_data_ids path (that's inlining-only).

Next: runtime-probe the value written to `perry_global_<mod>__55` at module-init (object vs closure) to confirm the assignment vs getter mistyping; investigate the IIFE-result local (`_cjs`, local 0) read at module-init scope at scale.

---
## ROOT (store-time probe, decisive)
`js_debug_val` injected at the module-global store (let_stmt.rs:785, gated PERRY_DBG_STORE on the COMPILE) shows the `"default"` Let (id 55) store-time value:
`[DEBUG_VAL] label=55 bits=0x7FFD045AB87A73B8` — tag `0x7FFD` = POINTER (runs once, deferred-init). Runtime `uw` is `typeof function`, so this pointer is the **closure**.

So `_cjs` (local 0, init = the IIFE `Call`) holds the **IIFE closure**, not the IIFE **call result** (the exports object), at store time — i.e. `const _cjs = (function(){...; return module.exports})()` binds `_cjs` to the *function* instead of its *return value*, ONLY at giant-bundle scale. The IIFE body's `module.exports` IS an object (PERRY_SCC), so the IIFE returns the object; the bug is the Call-result binding of `_cjs`. Not reproducible in ~14 minimal repros (incl. deferred lazy-require + -O3 auto-optimize) — a scale-emergent codegen defect in the IIFE-call-result assignment for a deferred cjs-wrapped module.

FIX area: the codegen that lowers `const x = (closure)()` (the cjs_wrap IIFE) — ensure `x` binds the Call RESULT, not the callee closure, under the giant-module / deferred-init path. Needs a scale reproduction or someone with the IIFE-call/deferred-init codegen context.

---
## store==load (definitive, same run)
`js_debug_val` at the store (let_stmt.rs) AND the importer getter-call (dyn_extern_i18n.rs:628), same run:
`label=55 (store) bits=0x7FFD02D428FA7130` == `label=9955 (load) bits=0x7FFD02D428FA7130` — IDENTICAL.
So the getter faithfully returns the stored value (NOT a load-side/getter bug, NOT corruption). `uw.name===""` (anonymous) ⇒ the stored value is the **IIFE function itself**, not the class (which would be `name==="SharedCacheControls"`). Definitive root: `const _cjs = (function(){...; return module.exports})()` binds `_cjs` to the IIFE **closure (callee)**, not the IIFE's **call result** (the exports object) — the IIFE body DOES run (module.exports populated) but its return value is discarded and the closure is stored. Giant-bundle-scale only (16+ repros incl. 150-module -O3 build all bind the call result correctly). The IIFE-call path is via the receiverless closure-value call (lower_call/console_promise.rs:997); it's correct in repros, so the defect is a scale-specific interaction (inlining / deferred-init / whole-program -O3) in the real bundle. Not reproducible synthetically → needs in-bundle debugging or the team's oversized-module codegen work.

---
## ROOT CONFIRMED (2026-06-19): deferred-require var captured by-value as a stale thunk
After exhaustively refuting prefix/global-id/FuncId collisions, -O3, GC, and the IIFE-return path (all clean), and tracing the value across the module boundary, the root is:

`uw = require("next/dist/server/lib/incremental-cache/shared-cache-controls.external.js")` is an **adopted/deferred require** (`cjs_wrap` rewrites `const uw = require('S')` → `import uw from 'S'`; `is_deferred_require` on the import decl). The `IncrementalCache` **constructor** (a class method inside app-page-turbo's cjs-wrap IIFE) **captures `uw` by value** (`js_closure_get_capture_f64`, NON-boxed; literals_vars.rs:434) at class-definition time — when `uw` is still the **unresolved thunk/closure**. So `new uw.SharedCacheControls(...)` reads a function → `uw.SharedCacheControls === undefined` → `TypeError: undefined is not a constructor` → HTTP 500.

### Verified value chain (one run, is_closure probe)
- perry_global store of the export = OBJECT (`is_closure=false`)
- importer getter-call result (the `uw` value via the getter) = SAME OBJECT (`is_closure=false`, identical bits)
- but the constructor's captured `uw` = FUNCTION (anonymous closure) — `W6X typeof=function`
- probe `js_dbg_closure_only` at the capture-read site: **47 by-value captures-of-closures in app-page-turbo**; `uw` is one (candidate ids 49 / 7499 / 7962 / 186xx).

### Why the boxing analysis misses it
`boxed_vars.rs:151` boxes a var only when `(declared AND captured AND mutated) OR self-recursive-closure`. `uw` is captured but assigned once (not "mutated"), so it's snapshotted by value. The **self-recursive-closure** rule (`collect_self_recursive_closure_ids`, boxed_vars.rs:148) is the exact precedent — it boxes `let f = closure()` because "the store happens AFTER captures populate." `uw`'s deferred require is the same "value not ready at capture" shape, just with a require/import init.

### Candidate fixes (delicate — import/capture subsystem; needs the repr decided first)
Whether `uw` is a `Stmt::Let` (boxed_vars-visible) or a pure adopted-import binding determines the site:
1. **box-when-captured**: if `uw` is a captured `Let` whose init is an adopted-require/import value → add to the boxed set (mirror the self-recursive rule), so the capture is by-reference and sees the resolved object. Verify the box actually receives the resolved value.
2. **eager-init**: a cjs-wrap-IIFE require runs at module init, so resolve it eagerly into the local before the class definition (then by-value capture = object).
3. **getter-on-read**: lower the constructor's `uw` read through the imported-var getter (`ExternFuncRef` → `perry_fn_<src>__<origin>`), consistent with init-scope reads, instead of a by-value capture.

### Repro status
NOT minimally reproducible (the eager-resolve path works in isolation; module-level require captured by a class method passes). Needs the cjs-wrap adopted/deferred-require + cross-module-getter shape — bundle-only so far. A faithful repro likely requires a compilePackage that cjs-wraps `const uw = require('dep'); module.exports.C = class { constructor(){ new uw.Thing() } }`.
