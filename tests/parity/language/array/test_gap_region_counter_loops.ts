// #10741: loop regions over arrays indexed by the loop COUNTER, with
// multi-statement bodies, element stores, pure Math calls and calls that can
// change the facts. The preheader guards each array once (dense raw-f64
// layout, integrity bits, `bound <= length`, the counter a non-negative
// integer); F-body reads and writes raw slots. Every case below breaks one
// of those facts mid-loop, or starts without it, and must print what node
// prints.

function mk(n: number, f: (i: number) => number): number[] {
  const a: number[] = [];
  for (let i = 0; i < n; i++) a.push(f(i));
  return a;
}

function show(label: string, v: unknown): void {
  console.log(label, JSON.stringify(v));
}

// The issue's particle step: five statements, two branches, compound
// assignment (its spilled base/key temps are copies of the binding/counter).
function step(x: number[], y: number[], vx: number[], vy: number[], n: number): number {
  for (let k = 0; k < n; k++)
    for (let i = 0; i < 64; i++) {
      x[i] += vx[i];
      y[i] += vy[i];
      if (x[i] < 0 || x[i] > 100) vx[i] = -vx[i];
      if (y[i] < 0 || y[i] > 100) vy[i] = -vy[i];
      vy[i] += 0.01;
    }
  let s = 0;
  for (let i = 0; i < 64; i++) s += x[i] + y[i];
  return Math.round(s * 1000);
}

function stepTyped(x: Float64Array, y: Float64Array, vx: Float64Array, vy: Float64Array, n: number): number {
  for (let k = 0; k < n; k++)
    for (let i = 0; i < 64; i++) {
      x[i] += vx[i];
      y[i] += vy[i];
      if (x[i] < 0 || x[i] > 100) vx[i] = -vx[i];
      if (y[i] < 0 || y[i] > 100) vy[i] = -vy[i];
      vy[i] += 0.01;
    }
  let s = 0;
  for (let i = 0; i < 64; i++) s += x[i] + y[i];
  return Math.round(s * 1000);
}

// Pure Math in the body (no JS can run): still bare.
function mathBody(a: number[], b: number[], n: number): number {
  for (let i = 0; i < n; i++) {
    const t = Math.sqrt(a[i] * a[i] + b[i] * b[i]);
    a[i] = Math.max(t, 1);
    b[i] = Math.floor(b[i] / 2) + Math.abs(a[i] - 3);
  }
  return a.reduce((p, c) => p + c, 0) + b.reduce((p, c) => p + c, 0);
}

// A call mutates the receiver mid-loop: shrinks it. The rest of this
// iteration and the next ones must see the shorter array.
let shrinkAt = 5;
function shrink(a: number[], i: number): void {
  if (i === shrinkAt) a.length = 8;
}
function callShrinks(a: number[], n: number): unknown[] {
  const out: unknown[] = [];
  for (let i = 0; i < n; i++) {
    a[i] = a[i] + 1;
    shrink(a, i);
    out.push(a[i]);
    a[i] = a[i] * 2;
  }
  return out;
}

// A call writes a string into the array (its raw-f64 layout is cleared).
function poison(a: number[], i: number): void {
  if (i === 3) (a as any)[i + 1] = "str";
}
function callPoisons(a: number[], n: number): unknown[] {
  const out: unknown[] = [];
  for (let i = 0; i < n; i++) {
    a[i] = a[i] + 1;
    poison(a, i);
    out.push(a[i] + 0);
  }
  return out;
}

// A call freezes the array: later stores are silently ignored (sloppy).
function freezer(a: number[], i: number): void {
  if (i === 2) Object.freeze(a);
}
function callFreezes(a: number[], n: number): number[] {
  for (let i = 0; i < n; i++) {
    a[i] = a[i] + 10;
    freezer(a, i);
  }
  return a;
}

// A call grows the array past its capacity (the elements move).
function grow(a: number[], i: number): void {
  if (i === 4) for (let j = 0; j < 100; j++) a.push(j);
}
function callGrows(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    a[i] = a[i] + 1;
    grow(a, i);
    s += a[i];
  }
  return s + a.length;
}

// A call replaces an element's prototype path: a hole read after the call.
function holePunch(a: number[], i: number): void {
  if (i === 1) delete (a as any)[6];
}
function callHoles(a: number[], n: number): unknown[] {
  const out: unknown[] = [];
  for (let i = 0; i < n; i++) {
    a[i] = a[i] + 1;
    holePunch(a, i);
    out.push(a[i]);
  }
  return out;
}

// An exception thrown mid-loop: the stores before it are visible after.
function thrower(i: number): void {
  if (i === 6) throw new Error("boom at " + i);
}
function throwsMidLoop(a: number[], n: number): string {
  try {
    for (let i = 0; i < n; i++) {
      a[i] = a[i] * 3;
      thrower(i);
      a[i] = a[i] + 1;
    }
  } catch (e) {
    return (e as Error).message + " " + JSON.stringify(a);
  }
  return "no throw";
}

// The counter written in the body: not a region index.
function counterInBody(a: number[], n: number): number[] {
  for (let i = 0; i < n; i++) {
    a[i] = a[i] + 1;
    if (a[i] > 4) i++;
  }
  return a;
}

// The bound written in the update clause and in the condition.
function boundInUpdate(a: number[], n: number): number[] {
  for (let i = 0; i < n; i++, n--) a[i] = a[i] + 1;
  return a;
}
function boundInCondition(a: number[], n: number): number[] {
  let m = n;
  for (let i = 0; i < (m = m - 1); i++) a[i] = a[i] + 1;
  return a;
}

// The array reassigned in the update clause: never a region receiver.
function arrayInUpdate(a: number[], b: number[], n: number): number[] {
  let c = a;
  for (let i = 0; i < n; i++, c = b) c[i] = c[i] + 100;
  return a.concat(b);
}

// Bound larger than the array: out-of-range reads are undefined, and a
// store past the end grows it (the guard fails; today's loop runs).
function pastEnd(a: number[], n: number): unknown[] {
  const out: unknown[] = [];
  for (let i = 0; i < n; i++) {
    out.push(a[i]);
    a[i] = i;
  }
  out.push(a.length);
  return out;
}

// A non-integer and a negative start: `a[0.5]`, `a[-1]` are properties.
function oddStarts(a: number[]): unknown[] {
  const out: unknown[] = [];
  for (let i = 0.5; i < 4; i++) out.push(a[i]);
  for (let i = -2; i < 3; i++) {
    out.push(a[i]);
    a[i] = i * 2;
  }
  out.push((a as any)[-1], (a as any)[-2]);
  return out;
}

// Holes from `new Array(n)` and a mixed-element array (no raw-f64 layout).
function holesAndMixed(): unknown[] {
  const h = new Array(6);
  h[0] = 1;
  h[3] = 4;
  const m: any[] = [1, "2", 3, null, 5];
  const out: unknown[] = [];
  for (let i = 0; i < 6; i++) {
    out.push(h[i]);
    h[i] = (h[i] ?? 0) + i;
  }
  for (let i = 0; i < 5; i++) {
    out.push(m[i] + 1);
    m[i] = m[i] + 1;
  }
  return out.concat(h, m);
}

// NaN through a Float64Array (its slots may hold any NaN payload) and into
// a plain array.
function nanFlow(t: Float64Array, a: number[], n: number): unknown[] {
  for (let i = 0; i < n; i++) {
    a[i] = t[i] * 2;
    t[i] = a[i] - 1;
  }
  return [Array.from(t).map(String), a.map(String)];
}

// A typed array of another kind, and a typed view over a shared buffer.
function otherKinds(n: number): unknown[] {
  const i32 = new Int32Array(n);
  const buf = new ArrayBuffer(8 * n);
  const v1 = new Float64Array(buf);
  const v2 = new Float64Array(buf);
  for (let i = 0; i < n; i++) {
    i32[i] = i * 3;
    v1[i] = i + 0.5;
    v2[i] = v1[i] * 2;
  }
  return [Array.from(i32), Array.from(v1), Array.from(v2)];
}

// Moving collections: the body allocates through a call every few
// iterations, so the young arrays move between iterations; the region must
// re-derive their element base.
let sink: unknown[] = [];
function churn(i: number): void {
  if (i % 7 === 0) {
    const junk: number[][] = [];
    for (let j = 0; j < 400; j++) junk.push([j, j + 1, j + 2]);
    sink = junk;
  }
}
function movingGc(n: number): number {
  let total = 0;
  for (let round = 0; round < 30; round++) {
    const a = mk(n, (i) => i + round);
    const b = mk(n, (i) => i * 2);
    for (let i = 0; i < n; i++) {
      a[i] = a[i] + b[i];
      b[i] = a[i] - 1;
      churn(i);
      a[i] = a[i] + b[i];
    }
    for (let i = 0; i < n; i++) total += a[i] + b[i];
  }
  return total + sink.length;
}

const X = mk(64, (i) => (i * 37) % 100);
const Y = mk(64, (i) => (i * 53) % 100);
const VX = mk(64, (i) => ((i * 7) % 11) - 5 + 0.25);
const VY = mk(64, (i) => ((i * 13) % 9) - 4 + 0.5);
show("step", step(X.slice(), Y.slice(), VX.slice(), VY.slice(), 300));
show(
  "stepTyped",
  stepTyped(Float64Array.from(X), Float64Array.from(Y), Float64Array.from(VX), Float64Array.from(VY), 300),
);
show("math", mathBody(mk(16, (i) => i - 4), mk(16, (i) => 7 - i), 16));
show("callShrinks", callShrinks(mk(12, (i) => i), 12));
show("callPoisons", callPoisons(mk(8, (i) => i * 1.5), 8));
show("callFreezes", callFreezes(mk(6, (i) => i), 6));
show("callGrows", callGrows(mk(8, (i) => i), 8));
show("callHoles", callHoles(mk(9, (i) => i), 9));
show("throws", throwsMidLoop(mk(10, (i) => i), 10));
show("counterInBody", counterInBody(mk(10, (i) => i), 10));
show("boundInUpdate", boundInUpdate(mk(10, (i) => i), 10));
show("boundInCondition", boundInCondition(mk(10, (i) => i), 10));
show("arrayInUpdate", arrayInUpdate(mk(5, (i) => i), mk(5, (i) => 10 * i), 5));
show("pastEnd", pastEnd(mk(4, (i) => i + 1), 7));
show("oddStarts", oddStarts(mk(5, (i) => i * 10)));
show("holesAndMixed", holesAndMixed());
const T = new Float64Array([1, NaN, -0, Infinity, 2.5]);
show("nanFlow", nanFlow(T, mk(5, () => 0), 5));
show("otherKinds", otherKinds(5));
show("movingGc", movingGc(50));
