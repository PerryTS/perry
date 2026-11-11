// A one-expression arrow declared in a loop of module code and only called
// in that loop is reduced to its body. Every other use keeps the closure.
class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

// Called once per iteration: reduced.
let sum = 0;
for (let i = 0; i < 5; i++) {
  const p = new P(i, i + 1);
  const f = () => p.x + p.y;
  sum += f();
}
console.log("once", sum);

// Used as a value as well as called.
let same = 0;
let viaValue = 0;
for (let i = 0; i < 3; i++) {
  const p = new P(i, 1);
  const f = () => p.x;
  if (f === f) same++;
  viaValue += f() + (typeof f === "function" ? 1 : 0);
}
console.log("value", viaValue, same);

// `this` of an arrow in module code.
const kinds: string[] = [];
for (let i = 0; i < 1; i++) {
  const f = () => this;
  kinds.push(typeof f());
}
console.log("this", kinds.join());

// `arguments` in a function's loop is the function's.
function args(a: number, b: number): number {
  let s = 0;
  for (let i = 0; i < 2; i++) {
    const f = () => arguments.length + arguments[0];
    s += f();
  }
  return s + b * 0;
}
console.log("arguments", args(5, 6));

// Recursion through its own binding.
let rec = 0;
for (let i = 0; i < 4; i++) {
  const f = (n: number): number => (n > 0 ? f(n - 1) + 1 : 0);
  rec += f(i);
}
console.log("recursion", rec);

// A captured local written after the declaration and before the call.
let later = 0;
for (let i = 0; i < 3; i++) {
  let k = i;
  const f = () => k * 10;
  k = 7;
  later += f();
}
console.log("written later", later);

// Called before its declaration: the call throws.
let tdz = 0;
for (let i = 0; i < 2; i++) {
  try {
    tdz += g();
  } catch (e) {
    tdz += 100;
  }
  const g = () => i;
  tdz += g();
}
console.log("tdz", tdz);

// Stored, then called twice.
const stored: (() => number)[] = [];
for (let i = 0; i < 3; i++) {
  const p = new P(i, 1);
  const f = () => p.x;
  stored.push(f);
}
console.log("stored", stored.map((h) => h() + h()).join());

// Captured by a closure that outlives the iteration.
const outer: (() => number)[] = [];
for (let i = 0; i < 2; i++) {
  const f = () => i + 1;
  outer.push(() => f() * 10);
}
console.log("captured", outer.map((h) => h()).join());

// Used from a function declared in the loop (hoisted out of the list).
let hoisted = 0;
for (let i = 0; i < 3; i++) {
  const f = () => i * 2;
  function h(): number {
    return f() + 1;
  }
  hoisted += h();
}
console.log("hoisted", hoisted);

// Each call makes its own object.
let fresh = 0;
for (let i = 0; i < 2; i++) {
  const f = () => ({ v: i });
  if (f() === f()) fresh += 100;
  fresh += f().v;
}
console.log("fresh", fresh);

// A parameter written in the body, an argument written elsewhere.
let params = "";
for (let i = 0; i < 2; i++) {
  let x = i;
  const f = (a: number) => (a = a + 10);
  const r = f(x);
  let m = 1;
  const bump = () => (m = 2);
  const h = (a: number) => bump() * 0 + a;
  params += r + "," + x + "," + h(m) + ";";
}
console.log("params", params);

// Async and generator arrows and functions.
async function asyncLoop(): Promise<number> {
  let s = 0;
  for (let i = 0; i < 2; i++) {
    const f = async () => i;
    s += await f();
  }
  return s;
}
function* gen(): Generator<number> {
  for (let i = 0; i < 2; i++) {
    const f = () => i * 3;
    yield f();
  }
}
console.log("generator", [...gen()].join());
asyncLoop().then((v) => console.log("async", v));
