// A generator partly consumed by for-of (break closes it), one consumed by
// manual next() first and then for-of, and a for-of that ends by exhaustion.
function* range(a: number, b: number): Generator<number> { for (let i = a; i < b; i++) yield i; }
const p = range(0, 6);
const s: number[] = [];
for (const v of p) { s.push(v); if (v === 2) break; }
console.log("closed", s.join(","), JSON.stringify(p.next()));
const q = range(0, 6);
console.log("manual", JSON.stringify(q.next()), JSON.stringify(q.next()));
const t: number[] = [];
for (const v of q) t.push(v);
console.log("rest", t.join(","), JSON.stringify(q.next()));
function* counted(): Generator<number, string> { yield 1; return "done"; }
const c = counted();
for (const v of c) console.log("c", v);
console.log(JSON.stringify(c.next()), JSON.stringify(c.return("x" as any)));
function* words(): Generator<string> { let w = "a"; for (let i = 0; i < 4; i++) { w += "b"; yield w; } }
const ws: string[] = [];
for (const w of words()) ws.push(w);
console.log("strings", ws.join(" "));
