### GC: an optional parameter lost its shadow slot — #7154's surviving residual (#7280)

`sfw-registry --help` under `PERRY_GC_MOVING_LOOP_POLLS=1` faulted in zod
`src/v4/core/util.ts`'s

```ts
export function clone<T>(inst: T, def?: T["_zod"]["def"], params?: { parent: boolean }): T {
  const cl = new inst._zod.constr(def ?? inst._zod.def);
  if (!def || params?.parent) cl._zod.parent = inst;
  return cl as any;
}
```

`params` is `Object(...)` in HIR and `type_is_pointer_bearing` says true, yet
`collect_pointer_typed_locals` gave it no slot. It lived in callee-saved `d8`
across `new inst._zod.constr(...)` — a user constructor crossing ~180 copying
minors — and `params?.parent` then dereferenced retired from-space.

**The cause is the optional marker, not the object type.** The pointer-locals
refinement fixpoint proves a local non-pointer from its **writes**, and for a
**parameter** the write list is a strict *subset* of its definitions: the
incoming argument is not a write. The optional-parameter desugaring then donates
the one write that completes the false proof, for free, on every optional
parameter in the program:

```
if (p === undefined) { p = undefined; }
```

`Void` is definitely-non-pointer, so "every write is non-pointer" held. Both of
that loop's conclusions are unsound for a parameter for the same reason, and
both are now excluded:

* `all_non_pointer` — fixes the declared-`Object` shape (zod's).
* `precise_inference` — fixes the `p?: any` shape, where the loop instead
  concluded `local_value_types[p] = Void` and any local **aliased from** `p`
  inherited it, was proven non-pointer, and lost *its* slot too. Measured red
  200/200 with only the first exclusion applied.

One-sided in the safe direction: a parameter that would have been proven
non-pointer keeps a root the collector rewrites harmlessly. Body `let`s are
untouched — their `Stmt::Let` init *is* in `writes`, so for them the write list
really is every definition.

**Found while bisecting toward that: four runtime-side siblings.** Every `new`
route codegen cannot resolve statically hands construction to perry-runtime,
and all four held the instance in a bare Rust local across the user constructor
body — `construct_registered_class_ref`, the class-object arm, the closure tail,
and `js_new_function_construct_with_new_target`. A runtime frame is not visited
by the precise root walk, so the helper returned the **pre-move address**.
Routed through `RuntimeHandleScope`, which is what this file's own
`CURRENT_NEW_TARGET` doc-comment already said needed doing; the same scope also
roots the displaced `prev_this` / `prev_new_target` / `prev_current_new_target`
values named in that comment.

Also corrects `gc/policy.rs`'s "sound by construction" claim for the loop-polls
route (#7280 ask 2). Deferring to `js_gc_loop_safepoint` makes the collection
point precise for **codegen** frames; it says nothing about a value parked in a
runtime Rust frame.

**Measurements** (release, `PERRY_GC_MOVING_LOOP_POLLS=1` at compile):

| arm | before | after |
|---|---|---|
| `sfw-registry --help`, `PROTECT_FROMSPACE=1 DEPTH=800` (no zeal) | FAULT 10/10 | **40/40 clean** |
| `sfw-registry --help`, plain polls | ~2/60 fail | **59/60** |
| 6 unit reproducers, `POLLS=1 ZEAL=1` | 200/200 iterations wrong | clean, `PERRY_GEN_GC=0` clean both sides |

Codegen cost, `sfw-registry` binary: **+33,088 bytes (+0.1231%)**, all of it from
the `all_non_pointer` exclusion; the `precise_inference` exclusion and the
runtime fix are +0.

Two new witnesses, both registered in `test-parity/gc_repsel_corpus.txt`:
`test_gap_gc_optional_param_receiver_rooting` and
`test_gap_gc_dynamic_construct_receiver_rooting`.

**#7280 stays open.** The plain arm is 59/60, not 60/60 — the surviving failure
is a `TypeError: Cannot read properties of undefined (reading 'has')`, a
different symptom from the pre-fix `object is not a function` / SIGSEGV, so at
least one more unrooted holder remains on that workload. **#7161's stopgap must
stay** until that one is found.
