// Products and their sums whose operands may be a String, a BigInt or an
// object with valueOf: the generic `+` / `*` stays (concatenation, BigInt
// arithmetic, TypeError on mixing, user code in source order).
class P {
  a: any; b: any;
  constructor(a: any, b: any) { this.a = a; this.b = b; }
  prod(): any { return this.a * this.b; }
  sumprod(s: any): any { return s + this.a * this.b; }
  chain(o: any): any { return this.a * this.b + o.c * 1 + o.c; }
  pre(o: any): any { return this.a + this.b + o.c; }
}
const arg = process.argv[2] ?? String(7 + Math.floor(Math.random() * 0.5));
const log: string[] = [];
const vo = { valueOf() { log.push("vo"); return 3; } };
const ob = { get c() { log.push("c"); return 5; } };
const ps = [new P(2, 3), new P(undefined, 1), new P(null, 2), new P(true, 1),
  new P(BigInt(arg), 2n), new P(BigInt(arg), 2), new P(vo, 4), new P(arg, 2), new P(arg, vo)];
const res: string[] = [];
for (const p of ps) {
  const fs = [() => p.prod(), () => p.sumprod(arg), () => p.sumprod(1), () => p.chain(ob), () => p.pre(ob)];
  for (const f of fs) {
    try { const r = f(); res.push(typeof r + ":" + String(r)); } catch (e) { res.push("E:" + (e as Error).name); }
  }
}
console.log(res.join("|"));
console.log(log.join(","));
const big = BigInt(arg) * 2n + 1n;
console.log(String(big), typeof big);
const u: any = [undefined, null, true][Math.floor(Math.random() * 0.5)];
const n: any = [null][Math.floor(Math.random() * 0.5)];
const t: any = [true][Math.floor(Math.random() * 0.5)];
console.log(String(u + 1), String(n * 2), String(t + 1));
