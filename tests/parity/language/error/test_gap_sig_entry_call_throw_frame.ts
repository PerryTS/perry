// KEEP: typed entries that must stay calls: a throwing body keeps its frame, a method overwritten by argv,
// two possible bodies, recursion
class P { x: number; constructor(x: number) { this.x = x; }
  put(a: number, b: number) { if (a > 1e9) throw new RangeError("big " + b); this.x = a + b; return this; }
  add(a: number, b: number) { return this.x + a * b; }
  fact(n: number): number { return n <= 1 ? 1 : n * this.fact(n - 1); } }
class Q { x: number; constructor(x: number) { this.x = x; } add(a: number, b: number) { return this.x - a - b; } }
class U { p: P; constructor(p: P) { this.p = p; } step(a: number) { return this.p.put(a, 2).x; } }
const arg = process.argv[2] || "7";
const p1 = new P(1); const p2 = new P(2); const u = new U(p1);
if (arg === "7") (p2 as any).add = function (a: number, b: number) { return -a - b; };
let t = 0; for (let i = 0; i < 1000; i++) { t += p1.add(i, 2) + p2.add(i, 3) + u.step(i); }
console.log("over", t);
const pick: P | Q = String(Math.random()).length > 100 || arg === "3" ? new Q(5) : new P(5);
let w = 0; for (let i = 0; i < 1000; i++) { w += pick.add(i, 1); }
console.log("two", w);
try { console.log(u.step(arg === "7" ? 2e9 : 5)); } catch (e: any) { console.log("err", e instanceof RangeError, e.message); const fr = String(e.stack).split("\n").slice(1, 2).map((l: string) => l.trim().split(" ")[1]); console.log("frames", fr.join(",")); }
console.log("rec", p1.fact(Number(arg) + 5));
