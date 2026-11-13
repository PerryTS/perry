// A copy of a local is the same value wherever it is read: in loop bodies,
// branches and closures, written by hand or left behind by an inlined call.
class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

function len(p: P): number {
  return p.x + p.y;
}

function bump(p: P): number {
  p.x = p.x + 100;
  return p.x;
}

// An inlined call in a loop.
let s = 0;
for (let i = 0; i < 1000; i++) {
  const p = new P(i, i + 1);
  s += len(p);
}
console.log("inlined", s);

// A hand-written copy; a store through either name is one store.
let t = 0;
for (let i = 0; i < 5; i++) {
  const p = new P(i, 1);
  const q = p;
  q.x = q.x + 10;
  p.y = p.y + 1;
  t += p.x * 100 + q.y;
  console.log("alias", i, p.x, q.x, p.y, q.y, p === q);
}
console.log("alias total", t);

// A callee that writes through its parameter writes the caller's object.
for (let i = 0; i < 3; i++) {
  const p = new P(i, 0);
  const r = bump(p);
  console.log("bump", r, p.x);
}

// A chain of copies, read in a nested loop and a branch.
for (let i = 0; i < 3; i++) {
  const a = new P(i, 2);
  const b = a;
  const c = b;
  let u = 0;
  for (let j = 0; j < 3; j++) {
    if (j % 2 === 0) {
      u += c.x;
    } else {
      u += b.y;
    }
  }
  console.log("chain", u);
}

// A copy that a closure captures keeps its own binding.
const readers: Array<() => number> = [];
for (let i = 0; i < 3; i++) {
  const p = new P(i, i);
  const q = p;
  readers.push(() => q.x + q.y);
}
console.log("captured", readers.map((f) => f()).join(","));

// A reassigned source or copy is two bindings.
for (let i = 0; i < 2; i++) {
  let p = new P(i, 0);
  const q = p;
  p = new P(50, 0);
  console.log("rebound source", p.x, q.x);
  let m = q;
  m = p;
  console.log("rebound copy", m.x, q.x);
}

// A read before the copy's declaration still throws.
for (let i = 0; i < 2; i++) {
  const p = new P(i, 0);
  try {
    const early = (q as P).x;
    console.log("no throw", early);
    const q = p;
  } catch (e) {
    console.log("early read", (e as Error).name);
  }
}

// A jump into a later switch case skips an earlier case's declaration.
function pick(n: number): string {
  try {
    switch (n) {
      case 0:
        const p = new P(1, 2);
      case 1:
        const q = p;
        return "case " + n + " " + q.x;
    }
    return "none";
  } catch (e) {
    return "case " + n + " " + (e as Error).name;
  }
}
console.log(pick(0), pick(1), pick(2));

// Copies of primitives.
let w = 0;
for (let i = 0; i < 4; i++) {
  const a = i * 2;
  const b = a;
  const label = "n" + i;
  const same = label;
  w += b + same.length;
}
console.log("primitives", w);
