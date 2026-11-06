// `for (const x of [e0, …])` over a short literal lowers to a counted loop
// over the evaluated elements when no iteration member can be patched
// (`perry-hir/src/lower/for_of_guard.rs`). Every observable of the
// IteratorRecord loop must hold.
const log: string[] = [];
function e(tag: string, v: number): number { log.push(tag); return v; }

// Elements evaluate once, in order, before the first iteration.
let sum = 0;
for (const d of [e("a", 1), e("b", 2), e("c", 3)]) { log.push("body" + d); sum += d; }
console.log(sum, log.join(","));

// break / continue / return.
function firstOver(limit: number): number {
  for (const v of [3, 7, 11, 13]) { if (v > limit) return v; }
  return -1;
}
let seen = "";
for (const v of [1, 2, 3, 4, 5]) { if (v === 2) continue; if (v === 4) break; seen += v; }
console.log(firstOver(8), firstOver(20), seen);

// A fresh binding per iteration: closures see their own value.
const fns: (() => number)[] = [];
for (const v of [10, 20, 30]) fns.push(() => v);
console.log(fns.map((f) => f()).join(" "));

// `let` may be reassigned inside the body; it does not affect the iteration.
let out = "";
for (let v of [1, 2, 3]) { v *= 10; out += v + ";"; }
console.log(out);

// Mixed values, strings and objects, nested loops.
const names: string[] = [];
for (const s of ["x", "y"]) for (const n of [1, 2]) names.push(s + n);
console.log(names.join(","), [...[1, 2]].length);
for (const o of [{ k: 1 }, { k: 2 }]) o.k += 1;

// Inside a class method, a generator, and with a labeled jump (fallback).
class Board {
  sq = [0, 0, 1, 0, 1, 0];
  attacked(at: number): boolean {
    for (const delta of [-1, 1]) { if (this.sq[at + delta] === 1) return true; }
    return false;
  }
}
const b = new Board();
console.log(b.attacked(1), b.attacked(3), b.attacked(0));
function* gen(): Generator<number> { for (const v of [5, 6]) yield v * 2; }
console.log([...gen()].join(","));
let labeled = "";
outer: for (const a of [1, 2, 3]) { for (const c of [1, 2]) { if (c === 2) continue outer; if (a === 3) break outer; labeled += a + "" + c + " "; } }
console.log(labeled);

// A throw leaves the loop the same way.
try { for (const v of [1, 2]) { if (v === 2) throw new Error("stop " + v); } } catch (err) { console.log((err as Error).message); }

// Assigning a `const` binding throws.
try { for (const v of [1]) { (v as any) = 2; } } catch (err) { console.log((err as Error).constructor.name); }
