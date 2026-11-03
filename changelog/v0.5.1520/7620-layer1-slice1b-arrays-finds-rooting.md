### Layer 1 rooting migration, slice 1b — `expr/arrays_finds.rs` + `expr/array_methods.rs` (#7615)

Both modules now root every operand through `crate::rooting` and are listed in
`MIGRATED_MODULES`; neither names `expr::temp_root`. Follows the template
(#7617) and slice 1a (#7618).

**A new combinator, with its callers.** `rooting::with_operands_rooted_across`
roots an operand group across a lowering step whose *representation* the caller
picks — here `u8[i]` / `buf[i]`, where the index goes through
`lower_expr_as_i32` or `fptosi` and the receiver is live across that choice.
`with_operands_rooted`'s re-read point is fixed at the end of its operand list,
so an operand lowered before caller-controlled work would be re-read above it
and stale again by the call, which is the #7114 half-measure. `across_exprs` is
passed as expressions rather than a `bool` so "does this window collect?" stays
inside `operand_protection`; `with_operands_rooted` is now the empty-`across`
case of it, keeping that decision in one place.

**Windows closed.** `Expr::BufferSlice` unboxed the receiver to a **raw**
`BufferHeader*` *before* lowering `start` and `end`, so `buf.slice(f(), g())`
read a pre-move address — and because what was stale was an `i64` derived above
the window rather than a `double`, #7280's `root_reload` structurally could not
repair it. The four `arr.find*` arms held the array in a register while the
callback was lowered, and a callback literal is a `js_closure_new`. Plus the
ordinary operand-to-operand windows: `AggregateError`, `Buffer.concat(list,
total)`, `Object.create(proto, props)`, both `FinalizationRegistry` mutators,
both `ErrorNew*` forms, `Object.is`, `Object.hasOwn`, `path.matchesGlob`,
`path.resolve`'s pairwise joins, the `Map`/`Set` positional readers, the
multi-argument `new Date(…)`, `NativeArenaView` / `NativePodView`, and the
polymorphic `u8[k] = v` store.

**Scoped honestly.** For a shadow-slotted local receiver #7280 had already
repaired the `find` window — visible in the baseline IR as an out-of-sequence
re-read, and accounting for all 16 removed `load double` instructions. What this
slice genuinely adds is the three shapes #7280 cannot cover: a receiver
reassigned by its own argument (where re-loading would observe the assignment,
so only a temp root gives both the call-time value and a rewritten address), a
raw already-unboxed pointer, and operands with no slot at all.

**Verified locally** (CI backlog is deep, so this is the evidence): IR
byte-identical up to register/label renaming on 8 purpose-built probes (172
functions, 164 identical, 8 differing — all `main`, net delta 100% root
plumbing, nothing deleted) and over the whole `gc-root-dominance` corpus (2452
functions, 2443 identical; 6 root-plumbing diffs and 3 that are pre-existing
compiler nondeterminism, proven by compiling one source twice with the same
binary). `gc-root-dominance` green in both gated modes with `--seeded-violations
40` at 40/40 and root stores up 9810 → 9826 (the gate's subject was live);
`-p perry-codegen --lib` 691 pass and `--doc`'s two `compile_fail,E0499` arms
still reject; `-p perry-runtime --no-fail-fast` 1886 pass on the first run; 60
gap tests over 17 family filters identical on both arms; probes byte-identical
again under `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` with the protector
confirmed live (5 retired sets/run). Ledger sabotage run per module: a real
`temp_root_*` pair turns the ledger red naming the exact lines, in both files.

**Reported, not fixed here.** `path.resolve(base, f())` throws
`ERR_INVALID_ARG_TYPE` where node returns the path: `Expr::PathResolveJoin`
unboxes with `unbox_to_i64`, so a short computed string's inline SSO bytes are
read as a `StringHeader*` (#214 class, bisected by string length, present on
both arms). The correct helper allocates, which opens the #7213 window in the
same arm — a rooting change with its own combinator question, not something to
hide inside a refactor.
