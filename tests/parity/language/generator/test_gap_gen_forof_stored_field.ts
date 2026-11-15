// A generator stored in a field and looped from two places: the second loop
// continues where the first stopped (break closes, so it sees nothing more),
// and a generator handed between two functions keeps one state.
function* nat(): Generator<number> { let i = 0; while (true) yield i++; }
class Holder { gen: Generator<number>; constructor() { this.gen = nat(); } }
const h = new Holder();
function take(x: Holder, n: number): number[] { const r: number[] = []; if (n === 0) return r; for (const v of x.gen) { r.push(v); if (r.length >= n) break; } return r; }
console.log("first", take(h, 3).join(","));
console.log("second", take(h, 3).join(","));
function* fin(): Generator<number> { yield 1; yield 2; yield 3; yield 4; }
const box = { g: fin() };
const one: number[] = [];
const two: number[] = [];
function a() { for (const v of box.g) { one.push(v); if (v === 1) { b(); } } }
function b() { for (const v of box.g) { two.push(v); if (v === 3) return; } }
a();
console.log("a", one.join(","), "b", two.join(","), JSON.stringify(box.g.next()));
