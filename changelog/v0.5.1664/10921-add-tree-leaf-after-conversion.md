Fixed `a + b + c` reading a leaf *after* a conversion the specification orders
before it (#10904).

`lower_guarded_numeric_add` fuses a whole `+` tree into one shared guard, which
evaluates every leaf before any addition. That is faithful only when the
specification also finishes every evaluation before the first conversion. For
`Add(L, R)` the spec evaluates `L`, evaluates `R`, and only then `ToPrimitive`s
both — so when `L` is itself an `Add`, its conversions run *before* `R` is
evaluated, and a user `valueOf`/`toString` inside `L` can change what a leaf in
`R` reads:

```js
const O = { a: null, b: 1, c: 7 };
O.a = { valueOf() { O.c = 100; return 1; } };
O.a + O.b + O.c;   // parses as (O.a + O.b) + O.c — node 102, perry 9
```

The rule is exact rather than a leaf-count approximation, and narrower than
"decline every left-leaning chain": `Add(L, R)` is faithful iff `L` and `R` are,
and, when `L` is an `Add`, **every leaf of `R` is evaluation-invariant**. A
property read is not invariant (a `valueOf` can assign `O.c`); a literal is, and
so is a `LocalGet` whose storage only this activation writes — outside
`boxed_vars` (captured and assigned anywhere, a parameter a sloppy mapped
`arguments` aliases, a TDZ box), not a module global, not a POD record. No new
analysis: `boxed_vars` already means "some other code can write it". Declining
every left-leaning chain instead cost `x + y + z` over plain locals — one of the
commonest expressions in JavaScript — for no correctness gain (lane 13's
`read4_stmt` column was +91..+99).

Soundness of the exemption: the cold arm (`rebuild_add_tree(fast = false)`)
already performs the conversions in spec order over the lowered values. The only
thing #10904 broke was *reading* a leaf before an earlier conversion could run,
and for an invariant leaf the read time is unobservable.

The check lives at the top of the fold, not at one call site.
`lower_guarded_numeric_add` is reached from two places, and gating only the
dynamic entry left the same stale read alive on the declared-number entry: with
`a: number[]` and an object in `a[0]` whose `valueOf` assigns `a[2]`,
`a[0] + a[1] + a[2]` printed 6 where node prints 103 — on the fix branch and on
main. A declined tree now lowers node by node through the spec helper from
either entry, and `dynamic_add_tree_benefits_shared_guard` returns to its
main-branch form: it answers whether the fold is *worth* it, not whether it is
*correct*.

The dominant accumulator shape keeps its guard: `sum += row.x + row.y` parses as
`sum + (row.x + row.y)`, which is right-leaning and faithful.
