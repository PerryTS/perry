// Issue #1081: expression-bodied closure whose body is a void call
// (e.g. `v => console.log(v)`) compiled to a WASM function whose
// signature returns i64. emit_expr for console.log/warn/error
// returns early without pushing a result; the synthesized
// `return <expr>` then leaves the operand stack empty, tripping the
// validator with "expected 1 elements on the stack for return, found 0".
//
// Fix lives in `Stmt::Return(Some(e))` — if the expression is known
// to be void (`expr_has_value` returns false), push `undefined`
// before emitting the WASM `return` so the function signature stays
// balanced.

async function g(): Promise<number> {
  return 42;
}

// Expression-bodied arrow whose body is a void call.
g().then(v => console.log(v));

// Same shape, multiple sites: void call as the *return* expression of
// an expression-bodied closure (the bug pattern from #1081).
const log = (v: number) => console.log(v);
log(1);
log(2);

console.log("done");
