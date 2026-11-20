### `perf(transform)`: eliminate redundant property reads across diverging guard chains

The discriminated-union dispatch idiom — the most common shape in idiomatic
TypeScript — reads its discriminant once per arm:

```ts
function evalNode(n: Node, env: Env): Value {
  if (n.kind === "num") return { kind: "num", num: n.num };
  if (n.kind === "str") return { kind: "str", str: n.str };
  if (n.kind === "var") return lookup(env, n.name);
  …
}
```

Every one of those reads compiled to a **full generic property-get diamond**:
the receiver-tag routing (`pget.recv_sso` / `pget.check_class_ref` /
`pget.recv_ok`), the monomorphic-IC guard ladder (five loads out of the object
header, two global loads, ~20 ALU ops), `pic.hit` / `pic.miss` / `pic.merge`,
and a four-way `phi` — about fifty instructions and six branches per site
(`expr/property_get/generic_dispatch.rs`).

LLVM `-O3` cannot remove them, and the reason is structural rather than a
missing attribute: GVN dedupes *instructions*, not congruent control-flow
regions, and every arm of the diamond that is not the inline fast load ends in
an opaque call (`js_object_get_field_ic_miss`,
`js_object_get_field_by_name_f64`) whose memory effects license nothing. So
the redundancy has to be removed before it becomes a diamond.

`perry_transform::prop_cse` does that. Inside a run of consecutive
`if (COND) { … }` statements with no `else` whose bodies **always diverge**
(`return` / `throw` / `break` / `continue`), the fall-through path from one
guard to the next runs no user code at all — only the guard expressions
themselves. A `local.property` read appearing in two or more guards of such a
run is therefore redundant, and one hoisted `const` replaces all of them.
Divergence is what makes the run analyzable: a body that falls through could
call anything, and then the next guard's read is a genuinely fresh
observation.

The pass runs after the inliner and the loop unroller, so inlined callee
bodies get the same treatment.

**Where the hoist may go.** Not "the top of the run" — the hoisted read is
evaluated where the `Let` lands, so it must land somewhere the original
program already evaluated it *unconditionally*. Otherwise
`if (a.x === 1) return; if (b.y === 2) return;` would start dereferencing `b`
on the path where the first guard returns, and a null `b` would throw where it
previously did not. A key is a candidate only at the guard where it is that
guard's **first unconditionally-evaluated read** — first, so hoisting it in
front of the guard cannot reorder it past a sibling read that could throw
first; unconditional, so it is not under the right-hand side of `&&`/`||`/`??`,
which the original may skip. Both halves have a test.

**What gates it.** Everything a guard may contain is on a closed allowlist:
literals, `LocalGet`, `local.prop`, **strict** `===`/`!==`, `&&`/`||`/`??`,
`!`, `typeof`. Loose compares and relational operators are excluded because
they coerce, and coercion calls `valueOf`/`toString`; `Binary` is excluded
wholesale for the same reason; calls, `new`, assignments, `await` and index
reads are excluded outright.

That leaves exactly one way for user code to run between two reads of
`n.kind`: **`kind` could be an accessor**, and a stateful getter returning a
different value on its second invocation would observe the difference. So the
pass carries a whole-module veto — a class getter or setter, an object-literal
accessor (`js_object_define_accessor`), a call naming
`defineProperty`/`defineProperties`/`__defineGetter__`/`__defineSetter__`, or
a `Proxy` construction anywhere in the module turns it off entirely. This
mirrors `perry-codegen`'s `Ptr<Shape>` promotion, which is vetoed module-wide
by the same family of constructs (`collectors/ptr_shape.rs`) before it emits
an *unguarded* field load — a strictly stronger assumption than this one. Two
residual holes are deliberate and named in the module docs:
`Object.create(proto, descriptors)`, and an object built by a getter-defining
module and passed in (the veto is per-module because the transform pipeline
is).

Async and generator bodies are skipped: those transforms run afterwards and
box every body local into a shared mutable `Any` cell, so a hoisted `const`
would become one more boxed cell rather than a register.

**Deliberately not covered yet**, each a straightforward follow-up rather than
a soundness question:

* `else if` chains. `if (a) … else if (b) …` nests as
  `If { else_branch: Some([If { … }]) }`, so each `else` list holds a run of
  one and nothing repeats within it. The same argument applies across the
  `else` edge — the else branch is reached exactly when an analyzable
  condition evaluated false, which ran no user code — so the run can be
  threaded through the nesting; it just needs a different traversal than the
  flat slice this version walks.
* Closure bodies. `Expr::Closure`'s body is a `Vec<Stmt>` the expression
  walker deliberately does not descend into, and this pass follows statements
  only.

`PERRY_PROP_CSE=0` disables the pass for bisection.

**Measured** (quiet M1 mini, best-of-5, `origin/main` @ `1ee158d27`, outputs
byte-identical to `node --experimental-strip-types` and exit-code checked;
both arms are the SAME binary with only the knob flipped, so nothing but the
pass differs):

| bench | pass off | pass on | |
|---|--:|--:|--:|
| `gc-handoff/apps/interp` | 1.4964 | **1.2424** | −17.0% |
| `gc-handoff/apps/iso_miss` | 1.9244 | **1.6819** | −12.6% |

The control arm is not a separate build: `pass off` is the same binary with
`PERRY_PROP_CSE=0`, and it reproduces the independently-built `main`
reference (`1ee158d27`) to within 0.2 ms — 1.4964 vs 1.4962 on `interp`,
1.9244 vs 1.9238 on `iso_miss` — so the delta is the pass and nothing else.

**The no-regression claim is a byte-comparison, not a timing run.** Compiling
the whole 25-program corpus twice from the same binary — once with the pass
on, once with `PERRY_PROP_CSE=0` — leaves **23 of 25 executables byte-identical**
(`churn`, `churn_alloc`, `churn_read`, `push_num`, `push_cls`, `cycles`,
`deeplist`, `tree`, `tree_wide`, `retain`, `retain1`, `retain_wide`,
`retain_wide1`, `fib40`, `asyncpipe`, `shapes`, `pipeline` and the six
bootstrap variants). The pass is a provable no-op for them, so their timing
cannot move — no host quiet enough to measure that is required. The two that
differ are the two that improved.

A full interleaved corpus run confirms it from the other direction. That run
was **not** taken under the shared-host lock, so its absolute seconds sit ~3%
high across every arm and are not quoted here — but the arms are interleaved
and 23 of the programs are byte-identical, so their `on/off` ratio *is* the
run's noise floor: it lands between **0.94 and 1.01** for every one of them.
`interp` (0.831), `interp_fo` (0.830) and `iso_miss` (0.875) sit far outside
that band, and reproduce the locked run's ratios (0.830 / 0.874) to ±0.001.

`interp_fo` — `interp.ts` with a `for (const _x of [1]) {}` prologue, which
forces the `globalThis` builtin bootstrap — gains exactly as much as `interp`,
and costs 0.14% over it.
Module-wide the generic-get site count in `iso_FIB.ts` falls from 238 to 208,
and `evalNode`'s emitted IR from 19102 to 17022 lines.

15 unit tests in `crates/perry-transform/src/prop_cse.rs` cover the rewrite,
each refusal (fall-through body, an intervening call, loose equality,
relational compare, a single occurrence, a short-circuited read, a key first
read by a later guard), the module veto in both directions — including an
accessor installed in a `for` **init**, which is a `Stmt` the flat expression
walk cannot reach — and the async-body skip.
