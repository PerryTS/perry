// A one-expression arrow local that is only called is reduced to its body
// at each call. That is only the call's meaning when the body leaves its
// parameters alone and each argument has one value for the whole call.

// A parameter written in the body is the arrow's own binding: the caller's
// local keeps its value.
function paramAssign(): string {
  let x = 1;
  const f = (a: number) => (a = a + 10);
  const r = f(x);
  return r + "," + x;
}

function paramUpdate(): string {
  let x = 1;
  const f = (a: number) => a++ + a;
  const r = f(x);
  return r + "," + x;
}

// The body writes the argument's local through another closure before it
// reads the parameter: the parameter still holds the value at the call.
function argWrittenThroughCall(): number {
  let m = 1;
  const bump = () => (m = 2);
  const f = (a: number) => bump() * 0 + a;
  return f(m);
}

// The argument's local is written after an effect the body runs first.
class Counter {
  n = 1;
  next(): number {
    this.n = this.n + 1;
    return this.n;
  }
}
function argReadAfterEffect(): string {
  let seen = 0;
  const c = new Counter();
  const note = () => (seen = c.next());
  const f = (a: number) => note() * 0 + a * 100;
  const r = f(seen);
  return r + "," + seen;
}

// Arguments in a different order than the body reads them.
function argsOutOfOrder(): string {
  let x = 3;
  let y = 4;
  const log: string[] = [];
  const touch = () => {
    log.push("t");
    x = 30;
    return 0;
  };
  const f = (a: number, b: number) => b - a + touch();
  const r = f(x, y);
  const g = (a: number, b: number) => b * 10 + a;
  const s = g(x, y);
  return r + "," + s + "," + x + "," + y + "," + log.join("");
}

// The same shapes with unwritten locals and literals, which may be reduced.
function stable(): string {
  const x = 5;
  const f = (a: number, b: number) => b * 10 + a;
  return f(x, 7) + "," + f(1, x);
}

// A module global written by a function is not a fixed value either.
let g = 1;
function setG(): number {
  g = 2;
  return 0;
}
function globalArg(): number {
  const f = (a: number) => setG() + a;
  return f(g);
}

console.log(paramAssign());
console.log(paramUpdate());
console.log(argWrittenThroughCall());
console.log(argReadAfterEffect());
console.log(argsOutOfOrder());
console.log(stable());
console.log(globalArg());

// The same shapes in a loop of module code.
let out = "";
for (let i = 0; i < 2; i++) {
  let x = 1;
  const f = (a: number) => (a = a + 10);
  const r = f(x);
  out += r + "," + x + ";";
  let m = 1;
  const bump = () => (m = 2);
  const h = (a: number) => bump() * 0 + a;
  out += h(m) + ";";
}
console.log(out);
