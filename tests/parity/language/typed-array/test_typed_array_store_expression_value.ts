// #7590: a typed-array element store used as an EXPRESSION must evaluate to
// the assigned value (ES2024 §13.15.2), not 0.
//
// The bug was that `ctx.discard_expr_value` — "this STATEMENT's value is
// discarded" — was not cleared as `lower_expr` recursed, so the store lowering
// saw it set while it was an OPERAND and returned 0.0. Every line below is an
// expression statement, so the flag is set, but the store's value is consumed
// in each; the discarded forms at the end must keep working too.
const buf = new Uint8Array(8);
const out: string[] = [];

function check(label: string, got: number, want: number): void {
  out.push(got === want ? label + ":ok" : label + ":WRONG got=" + got + " want=" + want);
}

// consumed as a call argument
check("arg", (buf[0] = 5), 5);
// consumed by an assignment
let n = 0;
n = buf[1] = 7;
check("assign", n, 7);
// consumed by arithmetic
check("binary", (buf[2] = 3) + 100, 103);
// consumed by a condition
check("ternary", (buf[3] = 1) > 0 ? 7 : 9, 7);
// consumed through a nested store
check("nested", (buf[4] = (buf[5] = 2)), 2);

// the ordinary discarded forms still store correctly
buf[6] = 11;
buf[7] = 12;

console.log(out.join(" "));
console.log("buf", buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]);
