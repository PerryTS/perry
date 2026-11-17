// KEEP: overwritten method by argv, two possible callees, a throwing callee, recursion
class A { v: number; constructor(v: number) { this.v = v; }
  get1() { return this.v + 1; }
  boom(o: any) { return o.q.length; }
  fact(k: number): number { return k <= 1 ? 1 : k * this.fact(k - 1); } }
class B { v: number; constructor(v: number) { this.v = v; } get1() { return this.v * 2; } }
const arg = process.argv[2] || "7";
const a1 = new A(3); const a2 = new A(4);
if (arg === "7") (a2 as any).get1 = function () { return -100; };
let t = 0; for (let i = 0; i < 1000; i++) { t += a1.get1() + a2.get1(); }
console.log("over", t);
const pick: A | B = String(Math.random()).length > 100 || arg === "3" ? new B(5) : new A(5);
let u = 0; for (let i = 0; i < 1000; i++) { u += pick.get1(); }
console.log("two", u);
try { console.log(a1.boom(arg === "7" ? null : { q: undefined })); } catch (e: any) { console.log("err", e instanceof TypeError, e.message); const fr = String(e.stack).split("\n").slice(1, 2).map((l: string) => l.trim().split(" ")[1]); console.log("frames", fr.join(",")); }
console.log("rec", a1.fact(Number(arg) + 5));
