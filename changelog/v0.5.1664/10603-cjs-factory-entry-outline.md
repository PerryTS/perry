perf(size): outline the CommonJS factory body, not just `hir.init` (#10575).
#8595's entry outliner only ever split `hir.init`, so a CommonJS module's body
was never outlined at all. `cjs_wrap::wrap_commonjs_for_target` wraps a CJS body
as *text* inside `function __perry_cjs_factory() { ... }`, nested in an
anonymous IIFE, before the parse/lower pipeline ever sees it. Lowering
represents that lexically-nested declaration the way it does any function
*expression* — a `Stmt::Let` naming an `Expr::Closure` inside `hir.init`'s own
expression tree, not a `hir.functions` entry — leaving `hir.init` itself with
only the handful of statements the wrapper adds at module scope (the `_cjs`
binding, `export default`, …). Automatic admission needs 1,000 top-level
statements or 4,000 estimated safepoints, so it could never fire on CJS no
matter how large the module was. On `typescript@5.9.3`'s `lib/_tsc.js` the real
body therefore stayed a single 463,716-instruction / 6.30 MB closure — past the
100,000-instruction optimized machine-pipeline budget, so the whole unit was
emitted through LLVM's O0 machine pipeline — and the linked binary contained
zero `__perry_entry_chunk_*` symbols.

`find_cjs_factory_closure`/`find_cjs_factory_closure_mut` locate that closure by
walking `hir.init`'s statement/expression tree. `outline_entry_module` now tries
`hir.init` first (unchanged #8595 behaviour) and, only if that is not a
candidate, falls back to the factory's body using the *identical*
`chunk_statements`/`analyze_stmts_outlining` machinery: same admission
thresholds, same fail-safe gates (top-level await, TDZ preallocation), same
`__perry_entry_chunk_*` naming, and each chunk still carries its origin body's
strictness. A module is only ever outlined from one origin per compile,
`hir.init` or the factory, never both. The factory is treated as a virtual
module entry only when it has the exact wrap-generated shape — no params, not
async, not a generator, and every id it captures resolving to a `Let` directly
in the enclosing IIFE — so failing the shape check is a defensive exit rather
than an expected one.

The one new concept is must-stay ids. The factory always captures exactly one id
from its enclosing scope: its own name. The preamble that opens the factory's
own body builds `const __cjs_module = { …, __perry_cjs_factory:
__perry_cjs_factory, … }`, a load-bearing self-reference that `perry-runtime`'s
`module_require.rs` reads back off the CJS record and invokes with
`js_closure_call0` on an already-loaded re-require path. A chunk is a plain,
non-capturing `hir.functions` entry and cannot read a captured id the way the
original closure could, so `classify_for_chunking` keeps any statement
referencing a captured id inline in the residual body, never relocating it into
a chunk, preserving the exact closure-capture codegen already emitted for it.
This is deliberately *not* solved by promoting the captured id to a module
global the way a cross-chunk `hir.init` let is: a global is one program-wide
instance, but a closure capture is fresh per invocation, so promoting it would
silently change re-invocation semantics on that recovery path. `hir.init`'s own
outlining passes an empty must-stay set, which short-circuits, so this is a
no-op for every non-CJS module. Alongside it, `emit_module_globals` folds an
outlined factory's logical statements into the same cross-chunk promotion it
already does for `hir.init`, so a `var` shared across the factory's new chunks
gets the same `@perry_global_*` treatment a cross-chunk `hir.init` `let` gets.

Measured on the issue's own repro, a full `typescript@5.9.3` build: the linked
binary now carries real chunk-derived symbols (`…_entry_chunk_…_0` through
`_19`, plus wrapper trampolines) where none existed before, and the single
463,716-instruction closure no longer appears anywhere in the build log.
Behaviour is unchanged — `--noEmit` over a type-error fixture is byte-identical
to `node node_modules/typescript/lib/_tsc.js` with the same exit code, and
`--version` still reports 5.9.3.

Honest caveat on size for this particular input: two individual chunks are still
around 190k and 396k instructions (down from one 463,716-instruction
whole-factory function), because a few of `_tsc.js`'s top-level statements are
themselves large enough to remain oversized even isolated into their own chunk —
sub-statement granularity is not attempted here — and the linked binary grew
from 86.4 MB to 100.0 MB, since ~20 function prologues and their GC-safepoint
scaffolding are not fully offset while a couple of chunks still hit the O0
fallback. Correctness is unaffected either way. A synthetic CJS fixture without
`_tsc.js`'s size-outlier statements went 11.0 MB → 9.2 MB and escaped the O0
fallback entirely.

Validated with `cargo test -p perry-codegen --lib` (1,598 passed), including
five new `entry_outline` tests: factory-shape matching and rejection
(param/async/generator/unresolvable-capture/wrong-name/no-wrapper), factory
outlining when `hir.init` is not a candidate, `hir.init` still winning when both
sides independently qualify, the self-reference-capture-stays-inline invariant,
and declining cleanly when neither side qualifies. The regression was proved
reachable rather than assumed: against pristine `origin/main`, a synthetic
1,200-statement CJS-factory-shaped module returned `Skipped("below automatic
outlining threshold")`. A synthetic 2,107-statement CJS fixture (cross-chunk
`var`s plus a closure created early and invoked from far-later statements)
produced output matching Node's own execution of the same file exactly, both
with outlining forced on and under `PERRY_OUTLINE_ENTRY=0` — where the disable
path reproduces the original 211,164-instruction / O0 pathology, confirming the
kill switch still works. `cjs_wrap_builtin_require` and
`issue_4872_barrel_default_reexports` pass unchanged, confirming small and
ordinary CJS modules are unaffected.
