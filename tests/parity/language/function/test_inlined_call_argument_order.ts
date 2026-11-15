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
