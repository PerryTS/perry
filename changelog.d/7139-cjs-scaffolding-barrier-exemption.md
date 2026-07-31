`Ptr<Shape>` rule 5 kills all shape promotion in any module containing a
`defineProperty`-family site, regardless of target. #7034 §2 recorded that
`Object.defineProperty(exports, "__esModule", { value: true })` — one line of
transpiler boilerplate — was doing this to half the dependency graph.

It was worse than that, and Perry was the one doing it. `cjs_wrap`'s preamble
emits `Object.defineProperty(require, 'name', { value: 'require', … })` into
**every** wrapped CommonJS module (`cjs_wrap/wrap.rs:841`, the shared
`cjs_preamble` used by both the IIFE wrap and the flat emit). So
`shape_barrier_sites` was true in **100 %** of the CommonJS dependency graph
before a single line of package source ran — and exempting the `__esModule`
marker alone would have recovered exactly nothing.

Both sites are now recognised, and only those two. A
`ObjectDefineProperty(target, key, desc)` node is exempt iff `target` is
`LocalGet(id)`, the module binds `id` with a `Stmt::Let` named `exports` (resp.
`require`) that has **no** `Expr::New` initializer anywhere, and `key` is the
string literal `"__esModule"` (resp. `"name"`). The `New`-init clause is the
soundness hinge: it is the exact negation of `find_new_candidates`' seed, so an
exempted target can never itself be a `Ptr<Shape>` local, and rule 2's
containment already keeps every promoted object out of any other binding. It is
checked on the HIR rather than assumed from the wrap template, so a future
template change degrades to *no exemption*, never to an unsound one.

Every other target, key, computed key, and barrier family (`delete`,
`setPrototypeOf` / `__proto__`, `Proxy`, mutating `Reflect.*`) keeps the
module-wide kill.

## Eligibility recovered, and promotion gained — separately

85 self-contained `__esModule` CJS leaves from `scriptc/node_modules`, compiled
as `compilePackages` dependencies by both arms (85/85 compiled in both):

| | before | after |
|---|---|---|
| modules with a rule-5 denial | **85** | **1** |
| `ptr-shape` denials, rule 5 (module barrier) | 117 | 14 |
| `ptr-shape` denials, rule 1 (provenance) | 154 | 154 |
| `ptr-shape` denials, rule 2 (containment) | 0 | **100** |
| distinct `ptr-shape` selections | **0** | 18 (in 2 modules) |
| `ptr-shape` consumptions | **0** | 16 (in 1 module) |

**Eligibility recovered: 84 of 85 modules (98.8 %).** The holdout has a genuine
barrier of its own.

**Promotion gained is small, and that is the finding.** Of the ~103 candidates
the barrier stopped denying, **100 were immediately re-denied by rule 2** —
containment, not the barrier, is the real wall in minified dependency code. The
18 that got through: 15 proven-`this` receivers in
`@eslint-community/regexpp` (16 consumption sites) and 3 anon-shape record
locals in `vscode-jsonrpc`'s `linkedMap.js` that are selected and consumed zero
times. Barrier narrowing is now done; the next win is containment (Track D).

## Adjacent question: does the barrier walk run before or after DCE?

After — but Perry performs no dead-code elimination in a default build, so the
walk sees everything. `collect_module_dispatch_facts`
(`perry-codegen/src/codegen/mod.rs:1385`) walks `hir.init`, every entry of
`hir.functions` whether called or not, and every class member body. The only
two dead-code passes, `reachability::tree_shake`
(`compile/reachability.rs:45`, module granularity) and
`env_fold::fold_env_branches` (`compile/env_fold.rs:30`, statically-false
`process.env` branches), are both gated on `ctx.tree_shake`, which defaults to
`false` (`compile/types.rs:1146`).

Quantified before acting: of **1750** barrier sites across 718 files in the
corpus, **12 (0.7 %)** sit inside a statically-dead `if`, and removing all of
them would take **zero** files from barrier-armed to barrier-free. Making the
walk DCE-aware would need an intra-module call graph and buys nothing
measurable, so it is recorded here rather than filed.

## Validation

`cargo test -p perry-codegen --lib` 416 passed (10 new, all sabotage-verified
red against the unfixed rule); census gate green with `batch` `ptr-shape` = 2
asserted against its floor; IR checked at the call sites — the promoted body
loses 3 `js_typed_feedback_class_field_get_guard`, 1 `…_set_guard`, 3
`js_typed_feedback_record_fallback_call`, 3 `js_object_get_field_by_name_f64`,
1 `js_method_direct_shape_guard` and 1 `js_native_call_method_by_id`, and drops
from 354 to 136 IR lines; behavioural A/B against Node 26.5.1 byte-identical on
both the synthetic probe and `regexpp` parsing/validating real patterns.
