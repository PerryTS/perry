### gc: the spread argument-bundle accumulator is rooted, and four of the residual hazards turn out to be checker false positives (#7664, #7453)

Six arms of `expr/call_spread.rs` bundle every argument — regular and spread, in
source order — into one JS array before dispatching (`console.*` spread, the
`recv.m(...)` and `recv[k](...)` method-apply arms, the namespace REST export,
and the closure-callee path's interleaved and multi-spread arms). All six wrote
the same loop and all six held the half-built array in a bare `i64` SSA register
across it:

```llvm
%acc  = call i64 @js_array_alloc(i32 0)
%box  = call double @perry_fn_…(…)                      ; arbitrary user code
%part = call i64 @js_array_like_to_array(double %box)   ; ALLOCATES
%acc2 = call i64 @js_array_concat(i64 %acc, i64 %part)  ; %acc is stale
```

`--statepoints --moving-only` reports that as `unrooted:alloc`: nothing in the
register's cast chain appears in the `js_array_like_to_array` safepoint's live
bundle, so an evacuating minor there neither marks nor rewrites the array and
`js_array_concat` reads from-space. It is #7453's shape — a fresh heap value in a
raw register across an allocating helper — in the spread lowering rather than the
URL one, and it is reachable from `a.splice(1, 0, ...src)`.

**The window is never empty here**, which is why the six sites share one helper
(`bundle_args_rooted`, a `rooting::with_rooted_accumulator` with the two fold
steps this lowering needs) rather than each asking `operand_protection`
separately. An `Expr::CallSpread` has at least one spread source by construction
and `js_array_like_to_array` allocates unconditionally, so `f(...[1, 2])` — every
operand an inert literal — is already the bug, and an operands-only "can anything
here collect?" predicate answers `false` for it. The `protect` predicate disjoins
the spread-present test for exactly that reason.

**Measured** on the native corpus (150 modules, 2538 functions, 32689
statepoints, 19593 live bundles), the same binary in both arms: **11 → 8**
`unrooted` hazards, `stale` still 0. The three that disappear are exactly the
three `unrooted:alloc` hits in `test_gap_array_splice_spread::main`; the other
eight are byte-identical. `gc-root-dominance-statepoints`'s `--max-unrooted`
drops to 8 in the same commit. All 71 gap tests containing a spread call are
byte-identical to the pinned Node 26.5.1 oracle.

The tests assert **ordering, never slot counts**, and walk *every*
`js_array_concat` rather than the first: on the default build the pooled-alloca
lowering emits a plain `store`/`load` and no `js_gc_temp_root_*` call at all, so
a `temp_root_calls(ir) > 0` assertion reads zero and passes vacuously; and a
bundle emits one fold per spread source, so checking only the first would leave a
partially-fixed loop green. The sabotage arm (`protect = false`, which reproduces
the pre-fix IR exactly) takes the suite to `0 passed; 3 failed`.

### ★ Four of the remaining eight hazards are checker false positives

The gate's comment described them as *"4 unmasked, all PHI-MEDIATED … the reload
has to go in the PREDECESSOR, on the edge, which is a different insertion
model"*. That is a fix for a problem that is not there, and the comment is
corrected rather than the code.

`"phi"` is in `TRANSPARENT_OPS`, so taint flows from a phi *operand* to the phi
*result* and the use is located at the join — but a phi operand is used on **its
own incoming edge**. All four hits are the same `&&` join, e.g. `readCtx`:

```llvm
entry.0:   %r2 = <unmask of the receiver>
           br i1 %r4, label %logical.then.1, label %logical.merge.2
logical.merge.2:
           %r87 = phi double [ %r2, %entry.0 ], [ %r86, %pget.recv_merge.7 ]
           ret double %r87
```

Every safepoint is on the `%logical.then.1` path, where the phi selects `%r86`;
on the edge that carries `%r2` nothing collects between its definition and the
join. Verified register-by-register on all four (`readCtx`, `__closure_5`,
`Readable`, `__obj_method_toLocaleString_3`).

So the real residual is **4**: two `@perry_global_*` reads
(`test_gap_arraybuffer_transfer::main`) that need rooting rather than reloading,
and two capture reads. The capture one in
`test_gap_class_expr_dynamic_parent_ctor::__closure_21` was checked by hand and
is real — capture 0 (the dynamic parent class) is read at the top of the closure
and passed to `js_new_function_construct` ~60 lines below, across
`js_object_alloc_class_inline_keys` **and** a user constructor, appearing in no
`gc-live` bundle anywhere in the function. It is **not** reloadable the way a
string-handle global is: the recipe would have to re-derive the closure pointer
from `%this_closure`, an `i64` *parameter* that RS4GC does not relocate, so the
re-read would address the pre-move closure. That population needs the callee's
own closure pointer to be a tracked root first.

An edge-sensitive phi rule is deliberately not included: it *lowers* a reported
count, so it must arrive with a sabotage arm proving it still reports a phi
operand that **is** live across a safepoint on its own edge.
