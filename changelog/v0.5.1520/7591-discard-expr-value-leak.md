**A typed-array element store used as an expression no longer evaluates to `0`
(#7590).**

```ts
const buf = new Uint8Array(4);
sink((buf[0] = 5), 5);           // was 0, now 5
n = buf[1] = 7;                  // was 0, now 7
sink((buf[2] = 3) + 100, 103);   // was 100, now 103
```

The stores themselves always landed correctly — only the expression's *value*
was wrong, so this was silent: a wrong number, no crash, no diagnostic. An
assignment expression must evaluate to the assigned value (ES2024 §13.15.2).

`ctx.discard_expr_value` means **"this STATEMENT's value is discarded"**. It is
set once per `Stmt::Expr` and `lower_expr` never cleared it while recursing —
the only reset in the tree is for constructor arguments
(`lower_call/new_ctor_args.rs`). Four sites read it as though it meant "this
EXPRESSION's value is discarded" and returned `double_literal(0.0)`, so they
fired while the store was an *operand* of the statement:
`expr/index_set.rs` (typed-array store, proven-view checked store) and
`expr/arrays_finds.rs` (`Uint8ArraySet` / buffer stores). `expr/dispatch.rs`
also reads the flag, but only to choose a materialization path, and is
unaffected.

The fix adds `FnCtx::discard_this_expr`, which `dispatch::lower_expr`
**takes** (`mem::take`) at the top of every dispatch. It therefore reaches
exactly one expression — the one the statement is made of — and every operand
lowered beneath it reads `false`. The handlers that need the answer receive it
as a parameter rather than reading the field, because they consult it *after*
lowering their operands, by which point the field has been taken again; reading
a field there would have reintroduced the same bug in a subtler form.

Found while trying to gate `arr.push(x)`'s length computation on the same flag
as a performance change: `js_array_length` is not a field read (it resolves
Proxy arrays through the `get` trap and probes the registered-Set/Map side
tables) and a statement-position push discards its result, which is 8–13% of
`push_cls` (see #7511). That optimisation produced this exact bug for
`sink(a.push(10))`, `n = a.push(20)`, `a.push(1) + 100` and
`a.push(1) > 0 ? 7 : 9`, which is what exposed the pre-existing one. The
optimisation itself is **not** included here — it wants this fix first, and
then the same non-leaking signal.

Regression test `test-files/test_typed_array_store_expression_value.ts` consumes
a store's value in call-argument, assignment, arithmetic, conditional and
nested-store position, and keeps two discarded stores to prove the ordinary
path still writes. That combination matters: the discarded form kept working
throughout, so a "does it still run" smoke test passes while the bug is live.

Verified against 78 `test-files` programs (typed-array/buffer weighted) — the
only behavioural difference is this test. `perry-codegen`'s failure set is
unchanged from `origin/main`.
