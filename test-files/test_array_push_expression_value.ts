// `arr.push(x)` used as an EXPRESSION must evaluate to the new length
// (ES2024 Array.prototype.push step 5), even though the statement-position
// form's length computation is elided as dead (#7592 follow-up to #7590).
//
// The elision is gated on the same `mem::take`n per-expression signal that
// #7591 introduced, so it reaches exactly the statement's own expression and
// never an operand. Every consuming position below would return 0 if that
// signal ever leaked into operand lowering again — and the discarded forms at
// the end must keep pushing correctly, which is why a "does it still run"
// smoke test cannot catch a regression here.
const out: string[] = [];

function check(label: string, got: number, want: number): void {
  out.push(got === want ? label + ":ok" : label + ":WRONG got=" + got + " want=" + want);
}

const a: number[] = [];
// consumed as a call argument
check("arg", a.push(10), 1);
// consumed by an assignment
let n = 0;
n = a.push(20);
check("assign", n, 2);
// consumed by arithmetic
check("binary", a.push(30) + 100, 103);
// consumed by a condition
check("ternary", a.push(40) > 0 ? 7 : 9, 7);
// consumed nested inside another push's argument
const b: number[] = [];
check("nested", b.push(a.push(50)), 1);
check("nested_value", b[0], 5);

// pointer elements: same expression positions through the all-pointer tier
const objs: { v: number }[] = [];
check("obj_arg", objs.push({ v: 1 }), 1);
let m = 0;
m = objs.push({ v: 2 });
check("obj_assign", m, 2);

// spread form consumed
const c: number[] = [1, 2];
const d: number[] = [3, 4, 5];
check("spread", c.push(...d), 5);

// the ordinary discarded forms still push correctly
a.push(60);
a.push(70);
objs.push({ v: 3 });
c.push(9);

console.log(out.join(" "));
console.log("a", a.length, a[0], a[4], a[5], a[6]);
console.log("objs", objs.length, objs[0].v, objs[2].v);
console.log("c", c.length, c[5]);

// a captured / boxed array takes the runtime fall-through path — cover it
let boxed: number[] = [];
const pushBoxed = (): number => boxed.push(1);
check("boxed_arg", pushBoxed(), 1);
boxed.push(2);
console.log("boxed", boxed.length);
