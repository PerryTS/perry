// Decision 69: loops (and straight-line runs) over a typed array with a
// COMPUTED length prove their indices against the length once, in a loop
// region guard, and re-check after anything that can run JS. These cases
// must print exactly what node prints: detach mid-loop, a resizable buffer
// shrinking mid-loop, reassignment, aliasing, out-of-range indices, and
// every typed-array kind.

const SIZES: number[] = [1, 2, 3, 4];
const N = SIZES.length * 2; // 8, a computed length

let detachAt = -1;
function maybeDetach(a: Float64Array, i: number): void {
  if (i === detachAt) (a.buffer as any).transfer();
}

// A spec clone (TaPtr parameter) with a counter loop and a constant bound.
function sumConst(a: Float64Array): number {
  let s = 0;
  for (let i = 0; i < 8; i++) {
    s += a[i] * 2 + a[7 - i];
  }
  return s;
}

// The same with a call inside the loop that may detach the buffer.
function sumDetach(a: Float64Array): string {
  const out: string[] = [];
  for (let i = 0; i < 8; i++) {
    maybeDetach(a, i);
    out.push(String(a[i]) + "/" + String(a[7 - i]));
  }
  return out.join(",") + " len=" + a.length;
}

// A rare call inside the loop: the facts go stale only on that path, the
// next iteration re-checks the length (a detach there leaves the region).
// The callee detaches the buffer through a module binding, so the loop's own
// binding never escapes and keeps its proven view.
const G = new Float64Array(SIZES.length * 2);
for (let i = 0; i < 8; i++) G[i] = i + 1;
function killG(): void {
  (G.buffer as any).transfer();
}
function sumMaybe(a: Float64Array, k: number, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = a[i];
    const w = a[i] + a[i] * 2 + a[i] * 3 + a[i] * a[i];
    s += v === undefined ? 1000 : w;
    if (i === k) killG();
  }
  return s;
}

// A counter that starts below zero: the guard's entry check must refuse it.
function fromStart(a: Float64Array, start: number): number {
  let s = 0;
  for (let i = start; i < 8; i++) {
    s += a[i] + a[i] * 2;
  }
  return s;
}

// An index with no range inside a guarded loop keeps its own check.
function withStray(a: Float64Array, n: number, k: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = a[i] + a[i] * 2 + a[i] * 3;
    const u = a[k];
    s += v + (u === undefined ? 1 : 0);
  }
  return s;
}

// A symbolic bound: `i < n` with `n` the caller's length.
function scaleTo(a: Float64Array, n: number, k: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    a[i] = a[i] * k;
    s += a[i];
  }
  return s;
}

// Nested loops with affine indices (the n-body shape).
function pairs(a: Float64Array): number {
  let e = 0;
  for (let i = 0; i < 4; i++) {
    const oi = i * 2;
    for (let j = i + 1; j < 4; j++) {
      const oj = j * 2;
      const d = a[oi] - a[oj];
      a[oi + 1] += d;
      a[oj + 1] -= d;
      e += d * d;
    }
  }
  return e;
}

// A straight-line run (no loop) of constant-index accesses.
function run(a: Float64Array): number {
  const x = a[0] + a[1];
  a[2] = x * 2;
  a[3] = a[2] + a[4];
  return a[3] + a[5] + a[6] + a[7];
}

// A straight-line run with a detaching call in the middle.
function runDetach(a: Float64Array): string {
  const x = a[0] + a[1];
  a[2] = x;
  maybeDetach(a, 0);
  const y = a[2];
  a[3] = 5;
  return String(x) + "," + String(y) + "," + String(a[3]) + "," + String(a[1]) + " len=" + a.length;
}

function fresh(): Float64Array {
  const a = new Float64Array(SIZES.length * 2);
  for (let i = 0; i < 8; i++) a[i] = i + 1;
  return a;
}

const a1 = new Float64Array(SIZES.length * 2);
for (let i = 0; i < 8; i++) a1[i] = i + 1;
console.log("const", sumConst(a1));
console.log("scale", scaleTo(a1, a1.length, 0.5));
console.log("pairs", pairs(a1).toFixed(6), Array.from(a1).join(","));
console.log("run", run(a1), Array.from(a1).join(","));

console.log("stray", withStray(a1, 8, 100), withStray(a1, 8, 3), withStray(a1, 8, -1));
console.log("from0", fromStart(a1, 0), "from-2", fromStart(a1, -2), "from1.5", fromStart(a1, 1.5));
console.log("maybe-none", sumMaybe(G, 99, G.length));
console.log("maybe-detach", sumMaybe(G, 4, 8), G.length);
const H = new Float64Array(SIZES.length * 2);
for (let i = 0; i < 8; i++) H[i] = i + 1;
function killH(): void {
  (H.buffer as any).transfer();
}
// A straight-line run with a detaching call in the middle.
function runKill(a: Float64Array): string {
  const x = a[0] + a[1];
  const y = a[2] + a[3];
  killH();
  const z = a[4];
  a[5] = 9;
  return x + "," + y + "," + z + "," + a[5] + " len=" + a.length;
}
console.log("run-kill", runKill(H));

const a2 = new Float64Array(SIZES.length * 2);
for (let i = 0; i < 8; i++) a2[i] = i + 1;
detachAt = 3;
console.log("detach-loop", sumDetach(a2));
detachAt = -1;

const a3 = new Float64Array(SIZES.length * 2);
for (let i = 0; i < 8; i++) a3[i] = i + 1;
detachAt = 0;
console.log("detach-run", runDetach(a3));
detachAt = -1;

// Out of range: the bound exceeds the length (the guard fails; plain loop).
const a4 = new Float64Array(SIZES.length * 2);
for (let i = 0; i < 8; i++) a4[i] = i + 1;
console.log("oob-scale", scaleTo(a4, 10, 2));
console.log("oob-detached", (() => { const b = fresh(); (b.buffer as any).transfer(); return scaleTo(b, 8, 2); })());

// A local fresh array with a computed length, detached by a callee mid-loop.
function localDetach(m: number): string {
  const b = new Float64Array(m * 2);
  for (let i = 0; i < b.length; i++) b[i] = i;
  let s = 0;
  const seen: number[] = [];
  for (let i = 0; i < m * 2; i++) {
    if (i === 2) (b.buffer as any).transfer();
    const v = b[i];
    seen.push(v === undefined ? -1 : v);
    s += i;
  }
  return seen.join(",") + " s=" + s + " len=" + b.length;
}
console.log("local-detach", localDetach(3));

// Reassignment of the array variable inside the loop.
function reassign(m: number): string {
  let b = new Float64Array(m * 2);
  const c = new Float64Array(m);
  for (let i = 0; i < b.length; i++) b[i] = i + 10;
  for (let i = 0; i < c.length; i++) c[i] = i + 100;
  const seen: number[] = [];
  for (let i = 0; i < m * 2; i++) {
    if (i === 2) b = c;
    const v = b[i];
    seen.push(v === undefined ? -1 : v);
  }
  return seen.join(",");
}
console.log("reassign", reassign(3));

// Aliasing: the same array through two parameters.
function alias(x: Float64Array, y: Float64Array): number {
  let s = 0;
  for (let i = 0; i < 8; i++) {
    x[i] = i * 3;
    s += y[i];
  }
  return s;
}
const a5 = new Float64Array(SIZES.length * 2);
console.log("alias", alias(a5, a5), alias(a5, new Float64Array(8)));

// A resizable ArrayBuffer shrinking mid-loop (a length-tracking view).
function shrink(): string {
  const rab = new (ArrayBuffer as any)(64, { maxByteLength: 128 });
  const v = new Float64Array(rab);
  for (let i = 0; i < v.length; i++) v[i] = i + 1;
  const seen: number[] = [];
  const n = v.length;
  for (let i = 0; i < n; i++) {
    if (i === 3) rab.resize(32);
    const x = v[i];
    seen.push(x === undefined ? -1 : x);
  }
  return seen.join(",") + " len=" + v.length;
}
console.log("shrink", shrink());

// Every typed-array kind, with a computed length, a loop and a run.
function kinds(m: number): string {
  const out: string[] = [];
  const make: Array<() => any> = [
    () => new Int8Array(m * 2),
    () => new Uint8Array(m * 2),
    () => new Uint8ClampedArray(m * 2),
    () => new Int16Array(m * 2),
    () => new Uint16Array(m * 2),
    () => new Int32Array(m * 2),
    () => new Uint32Array(m * 2),
    () => new Float32Array(m * 2),
    () => new Float64Array(m * 2),
  ];
  for (const mk of make) {
    const t = mk();
    for (let i = 0; i < t.length; i++) t[i] = i * 77 - 200;
    out.push(Array.from(t).join(" "));
  }
  return out.join(" | ");
}
console.log("kinds", kinds(4));

function i8(m: number): string {
  const t = new Int8Array(m * 2);
  for (let i = 0; i < m * 2; i++) t[i] = i * 70;
  let s = 0;
  for (let i = 0; i < m * 2; i++) s += t[i];
  t[0] = t[1] + t[2];
  t[3] = t[4] * 3;
  t[5] = t[6] - t[7];
  t[2] = t[0] + 200;
  return s + ":" + Array.from(t).join(",");
}
function u8c(m: number): string {
  const t = new Uint8ClampedArray(m * 2);
  for (let i = 0; i < m * 2; i++) t[i] = i * 70 - 100.5;
  let s = 0;
  for (let i = 0; i < m * 2; i++) s += t[i];
  return s + ":" + Array.from(t).join(",");
}
function u16(m: number): string {
  const t = new Uint16Array(m * 2);
  for (let i = 0; i < m * 2; i++) t[i] = i * 20000;
  let s = 0;
  for (let i = 0; i < m * 2; i++) s += t[i];
  return s + ":" + Array.from(t).join(",");
}
function i32(m: number): string {
  const t = new Int32Array(m * 2);
  for (let i = 0; i < m * 2; i++) t[i] = i * 1000000000;
  let s = 0;
  for (let i = 0; i < m * 2; i++) s += t[i];
  for (let i = 0; i < m; i++) { const o = i * 2; t[o] = t[o + 1] + 1; }
  return s + ":" + Array.from(t).join(",");
}
function u32(m: number): string {
  const t = new Uint32Array(m * 2);
  for (let i = 0; i < m * 2; i++) t[i] = i * 1000000000 - 1;
  let s = 0;
  for (let i = 0; i < m * 2; i++) s += t[i];
  return s + ":" + Array.from(t).join(",");
}
function f32(m: number): string {
  const t = new Float32Array(m * 2);
  for (let i = 0; i < m * 2; i++) t[i] = i / 3;
  let s = 0;
  for (let i = 0; i < m * 2; i++) s += t[i];
  t[0] = NaN;
  let nans = 0;
  for (let i = 0; i < m * 2; i++) if (t[i] !== t[i]) nans++;
  return s.toFixed(6) + ":" + nans;
}
function big(m: number): string {
  const t = new BigInt64Array(m * 2);
  for (let i = 0; i < m * 2; i++) t[i] = BigInt(i) * 3n;
  let s = 0n;
  for (let i = 0; i < m * 2; i++) s += t[i];
  return String(s);
}
console.log("i8", i8(4));
console.log("u8c", u8c(4));
console.log("u16", u16(4));
console.log("i32", i32(4));
console.log("u32", u32(4));
console.log("f32", f32(4));
console.log("big", big(4));
