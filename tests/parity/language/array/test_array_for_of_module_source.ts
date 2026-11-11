// `for (const x of e)` where `e` is a module-level binding, read from inside
// functions. The walk admits a Number array's packed loop in the same call as
// its iteration proof, and a `const x = a[k]` element binding sums unboxed in
// that loop; each case checks a way the module binding can stop being the
// array that proof saw.
const out: string[] = [];

// A module `const` Number array, summed by an outer loop many times.
const KNIGHT = [-33, -31, -18, -14, 14, 18, 31, 33];
function sumKnight(rounds: number): number {
  let s = 0;
  for (let i = 0; i < rounds; i++) for (const d of KNIGHT) s = s * 0.5 + d * (i + 1);
  return s;
}
out.push("k" + sumKnight(3));
function maxKnight(): number {
  let best = -Infinity;
  for (const d of KNIGHT) if (d > best) best = d;
  return best;
}
out.push("m" + maxKnight());

// The same const array after it changes: grown (its head moves), given a
// hole, given a non-Number element. Each walk must see the array as it is now.
const GROW = [1, 2, 3];
function sumGrow(): number {
  let s = 0;
  for (const d of GROW) s += d;
  return s;
}
out.push("g" + sumGrow());
for (let i = 4; i <= 40; i++) GROW.push(i);
out.push("g" + sumGrow());
const HOLEY = [1, 2, 3, 4];
function sumHoley(): string {
  let s = 0;
  let parts = "";
  for (const d of HOLEY) {
    s += d;
    parts += String(d) + ",";
  }
  return parts + s;
}
out.push("h" + sumHoley());
delete (HOLEY as any)[1];
out.push("h" + sumHoley());
const MIXED = [1, 2, 3];
function sumMixed(): number | string {
  let s: number | string = 0;
  for (const d of MIXED) s += d;
  return s;
}
out.push("x" + sumMixed());
(MIXED as any[]).push("z");
out.push("x" + sumMixed());
(MIXED as any[])[0] = "a";
out.push("x" + sumMixed());

// An accumulator that is not a Number when the loop starts.
function joinKnight(): string {
  let s: any = "";
  for (const d of KNIGHT) s += d;
  return s;
}
out.push("j" + joinKnight());

// A module `let` reassigned between calls (and to a non-array).
let current: any = [1, 2, 3];
function sumCurrent(): string {
  let s = 0;
  let n = 0;
  for (const d of current) {
    s += d;
    n++;
  }
  return n + ":" + s;
}
out.push("l" + sumCurrent());
current = [10, 20];
out.push("l" + sumCurrent());
current = new Set([5, 6, 7]);
out.push("l" + sumCurrent());
current = "abc";
out.push("l" + sumCurrent());
current = [100];
out.push("l" + sumCurrent());

// The element binding written in the body is not an element binding.
function rebind(): number {
  let s = 0;
  for (let d of KNIGHT) {
    if (d < 0) d = -d;
    s += d;
  }
  return s;
}
out.push("r" + rebind());

// Early exit from the walk over a module const.
function firstBig(limit: number): number {
  for (const d of KNIGHT) if (d > limit) return d;
  return 0;
}
out.push("f" + firstBig(15) + "," + firstBig(99));

// A module array whose Symbol.iterator is patched after the functions that
// walk it were compiled, and after they already ran once.
const PATCHED = [1, 2, 3];
function sumPatched(): number {
  let s = 0;
  for (const d of PATCHED) s += d;
  return s;
}
out.push("p" + sumPatched());
(PATCHED as any)[Symbol.iterator] = function* () {
  yield 40;
  yield 2;
};
out.push("p" + sumPatched());
out.push("k" + sumKnight(1));

console.log(out.join(" "));
