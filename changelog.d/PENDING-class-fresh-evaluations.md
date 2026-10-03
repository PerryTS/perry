**A class declaration evaluated more than once now creates a fresh class on every evaluation after the first, as Node does** (#11759).

`function f() { class K {} return K }` returned the same class on every call, so `f() === f()` was `true`, a property written to one evaluation's `prototype` showed up on the other's instances, `instanceof` answered `true` across evaluations, and every evaluation shared the same static fields. The same happened for a class declared in a module-level loop body. Classes that capture variables, have private elements, or have a computed key or a runtime `extends` value were already fresh per evaluation.

The fix follows owner decision 64, option (c′). The first evaluation of a declaration is still the shared class, so code that runs once keeps the static class and its fast paths: bundle wrappers (esbuild `__esm`/`__commonJS` bodies, which only the runtime can see running once), functions called once, and module top level. The second and later evaluations each create a fresh class object, built from #11780's template shapes.

- **The evaluation check.** Whether the first evaluation has happened is a fact of the class template: a module-private word per template (`@perry_class_first_eval.<template>`) that holds the shared class once the first evaluation has handed it out. An evaluation does one load, one compare and one branch. No word and no check is emitted where `lower::run_once` proves the declaration runs once: module top level outside loops, IIFE bodies, and functions called once from such a position.
- **The class's own name inside a repeatable declaration.** It resolves to that evaluation. The members read it from a guarded class environment, the scheme fresh class expressions use, so first-evaluation instances carry no capture fields.
- **Static forms where the binding holds the first evaluation.** `new C(...)` and `C.<static field>` through the binding are guarded on the first evaluation and take the static form there: a static `new` and a direct static-field load. The exact-receiver inliner treats the guarded `new` as constructing `C`; every evaluation runs C's constructor and carries C's declaration-time method table.
- **Subclasses in the same body.** `class D extends L`, where `L` is such a declaration in the same body, is one too. D's template extends L's template, and D shares its first evaluation only while L's binding holds L's first evaluation. Every other evaluation of D pins the evaluated `L` it extends.
- **Captured values.** The template-keyed capture snapshot (`RegisterClassCaptures`, read when the shared class is constructed by value) and its body-end refreshes belong to the first evaluation only. A later evaluation carries its own capture array.
- **Runtime.** A class function object has a second INT32 capture slot that records whether it is its declaration's first evaluation. Generated code sets it when the first evaluation hands the shared class out; the slot index and value are in `perry-abi`. From then on the shared class is a class of its own:
  - a later evaluation's static writes are no longer mirrored into it (#6530);
  - it no longer stands in for the template in `CLASS_OBJECT_VALUES`;
  - its static parent is no longer replaced by a later evaluation's parent;
  - `instanceof` against either evaluation, and `instance.constructor` of a later evaluation's instance, follow the instance's actual prototype chain.
- **Fixed on the way.** `monomorph`'s default-argument padding now reaches a `new` inside `ClassEnvStamp`. It also missed fresh class expressions' guarded `new K()`, which left captured values in omitted constructor parameters.

**Validation**

- **New gap tests.** `test_gap_11759_class_decl_fresh_per_evaluation`, `test_gap_11759_class_decl_loop_extends_wrapper` and `test_gap_11759_class_decl_evaluations_gc` cover identity, prototype isolation, statics, `delete`, captures, self-reference, instance fields, a module-top loop, `extends` of an evaluated class, a module-level parent, an esbuild `__esm` wrapper, a CommonJS factory a three-level chain with `super` calls and inherited statics over three evaluations, a subclass declared in a nested function, and 400 evaluations under GC pressure.
  - All three match Node 26.5.1. The GC test also matches under `PERRY_GC_MOVING_LOOP_POLLS`, `PERRY_GC_FORCE_EVACUATE`, `PERRY_GC_POISON_FROMSPACE` and a 64 KB allocation schedule.
  - All three fail on main and on a sabotaged build whose every evaluation is the shared class.
- **IR test.** `crates/perry/tests/issue_11759_class_first_evaluation.rs` checks that a repeatable declaration emits the check and fresh later evaluations, that a module-top loop body is repeatable, that an IIFE, a module-top block and a function called once emit neither, and that the esbuild wrapper keeps its one class.
- **Suites.** The full gap suite: the only differences from main are the three new tests passing. Runtime: 4846/0 (`RUST_TEST_THREADS=1`, release). `perry-hir`, `perry-transform` and `perry-codegen` tests pass. `gc_call_effects --check` (linux-x86_64) shows no drift from this change, and `--check-wasm-abi` is current.
- **Cost.** tsc and Zod are flat: Zod +0.01% instructions (n=5, 0/0 full collections, RSS 55.4/55.5 MB). tsc is within its bimodal noise: n=11, both builds split about evenly between the two modes, whose medians move +0.04% and −0.03%, 82/82 full collections, RSS 302.2/302.2 MB. `.text` is tsc −27 KB and Zod +3.9 KB.
  - A body that runs once but cannot be proven to, with a hot loop over its class (`new` + method + static field + static call): 215 → 309 instructions per iteration.
    - `new C()`: 35 → 90 (the guard and the lost scalar replacement);
    - static call: 82 → 121;
    - static field read: 30 → 35;
    - method call on the new instance: 7 → 11.
  - A later evaluation pays #11780's fresh-class path: evaluation + `new` + call is 31.1k instructions against 3.9k for the shared class; a static method adds ~660 per evaluation.
  - Of the repo files that evaluate such a declaration more than once:
    - `test_gap_10490_implicit_this_scope_rooting` (500 evaluations): +1.8%;
    - `test_gap_9466_shadowed_class_identity` (3 evaluations): +10.5% of a 6.4M-instruction run.
