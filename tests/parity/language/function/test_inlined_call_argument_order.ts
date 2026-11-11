// A call's arguments are evaluated before the callee's body runs. When the
// compiler inlines the callee, an argument that the body (or a later
// argument) can change must still be read at the call, not where the body
// uses the parameter.

let m = 1;
function bump(): number {
  m = m + 1;
  return 0;
}

// Expression body: the read of `a` comes after bump() in the body.
function g(a: number): number {
  return bump() * 0 + a;
}
console.log("expr body", g(m), m);

// Statement body, at module level.
function f(a: number): number {
  bump();
  return a;
}
const r = f(m);
console.log("stmt body", r, m);

// A later argument changes the earlier one.
function add2(a: number, b: number): number {
  return a + b;
}
function add2r(a: number, b: number): number {
  return b + a;
}
console.log("later arg", add2(m, bump()), m, add2r(m, bump()), m);

// A function-local `let` written by a closure the body calls.
function viaClosure(a: number, w: () => number): number {
  return w() * 0 + a;
}
function runLocal(): void {
  let x = 1;
  const w = (): number => {
    x = x + 10;
    return 0;
  };
  console.log("closure local", viaClosure(x, w), x);
}
runLocal();

// A parameter of the caller, written by a closure stored in a global.
let setP: (v: number) => number = (_v: number): number => 0;
function readAfterSet(a: number): number {
  return setP(5) * 0 + a;
}
function stmtAfterSet(a: number): number {
  setP(7);
  return a;
}
function caller(p: number): string {
  setP = (v: number): number => {
    p = v;
    return 0;
  };
  const first = readAfterSet(p);
  p = 1;
  const second = stmtAfterSet(p);
  return first + "," + second + "," + p;
}
console.log("caller param", caller(1));

// Method argument (expression body and void body).
class C {
  k: number;
  constructor(k: number) {
    this.k = k;
  }
  plus(a: number): number {
    return bump() * 0 + a + this.k;
  }
}
const c = new C(100);
console.log("method", c.plus(m), m);

class Acc {
  s = 0;
  add(a: number): void {
    bump();
    this.s = this.s + a;
  }
}
const acc = new Acc();
acc.add(m);
console.log("void method", acc.s, m);

// Object field argument and a const argument: unaffected controls.
const o = { x: 1 };
function bumpX(): number {
  o.x = o.x + 1;
  return 0;
}
function readField(a: number): number {
  return bumpX() * 0 + a;
}
console.log("field", readField(o.x), o.x);
const k = 7;
console.log("const", g(k), m);

// A written local passed to a body that reads its parameter before anything
// that could run code: the inlined body may read the local itself, also
// where a binding has no place (a loop condition).
function onBoard(sq: number): boolean {
  return sq >= 0 && sq < 128 && (sq & 0x88) === 0;
}
function firstRead(sq: number): boolean {
  return (sq & 0x88) === 0;
}
function ray(from: number, delta: number): number {
  let source = from + delta;
  let steps = 0;
  while (onBoard(source)) {
    source += delta;
    steps++;
  }
  return steps;
}
console.log("loop condition", ray(0, 1), ray(0x33, 0x11), ray(0x70, -0x10), ray(0x77, 1), ray(3, -1));
function rayAny(from: any, delta: number): number {
  let source = from;
  let steps = 0;
  while (firstRead(source) && steps < 20) {
    source += delta;
    steps++;
  }
  return steps;
}
console.log("loop condition, first read", rayAny(0, 1), rayAny(0x33, 0x11), rayAny("7", 1));
function square(a: number): number {
  return a * a;
}
function exprUse(): string {
  let w = 3;
  w = w + 1;
  const first = square(w) + (onBoard(w) ? 1 : 0);
  w = 0x88;
  return first + "," + onBoard(w) + "," + w;
}
console.log("expression", exprUse());
let top = 0x70;
do {
  top += 0x10;
} while (onBoard(top));
console.log("module loop", top, square(top));

// Controls: the read must stay at the call.
// A later argument writes the earlier one.
function laterWrite(): string {
  let v = 1;
  const a = add2(v, v++);
  const b = add2r(v, (v = 10));
  return a + "," + b + "," + v;
}
console.log("later arg writes", laterWrite(), add2(m, m++), m);

// A conversion runs code between two reads of the parameter.
let boxed: any = {
  valueOf(): number {
    boxed = 5;
    return 1;
  },
};
function twice(a: number): number {
  return (a & 1) + a;
}
console.log("conversion", twice(boxed as number), boxed);
// The same with a local that starts as a number and is stored an object.
function lateObject(): string {
  let v: any = 1;
  const before = twice(v as number);
  v = {
    valueOf(): number {
      v = 5;
      return 1;
    },
  };
  return before + "," + twice(v as number) + "," + v + "," + onBoard(v as number);
}
console.log("late object", lateObject());

// A getter runs between two reads of the parameter.
let holder: any = {
  get x(): number {
    holder = 7;
    return 1;
  },
};
function viaGetter(a: number): string {
  return (a as any).x + ":" + (a === 7 ? "moved" : "kept");
}
console.log("getter", viaGetter(holder as number), holder);

// A body that stores to the very local it was passed.
let shared = 1;
function storesFirst(a: number): boolean {
  return (shared = 9) === a;
}
console.log("store", storesFirst(shared), shared);

// A loop in the body that runs code: its reads are late.
function countTo(a: number): number {
  let n = 0;
  while (n < a) {
    n = n + 1 + bump();
  }
  return n;
}
m = 3;
console.log("body loop", countTo(m), m);

// Methods: the field is read after the parameter, or before it.
class Probe {
  k = 1;
  early(a: number): boolean {
    return a === this.k;
  }
  late(a: number): number {
    return bump() * 0 + a;
  }
}
function probes(): string {
  const p = new Probe();
  let q = 0;
  q = q + 1;
  let hits = 0;
  while (p.early(q)) {
    q += 1;
    hits++;
  }
  return hits + "," + q + "," + p.late(m) + "," + m;
}
console.log("methods", probes());
