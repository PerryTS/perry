// D2 negative: objects with valueOf/toString (and BigInt) reaching `+` and `*`
// through fields.
class Shape {
  x: any; y: any;
  constructor(x: any, y: any) { this.x = x; this.y = y; }
  area(): number { return 3; }
  sum(): any { return this.area() + this.x + this.y; }
  prod(): any { return this.x * this.y; }
}
const log: any[] = [];
const vo = { valueOf() { log.push("vo"); return 10; } };
const ts = { toString() { return "T"; } };
const cases: [any, any][] = [[1, 2], [vo, 2], [ts, 2], [1, ts], ["a", 2], [null, true], [undefined, 1], [2n, 3n], [vo, vo]];
const res: string[] = [];
for (const [a, b] of cases) {
  const s: Shape = new Shape(a, b);
  try { res.push(String(s.sum())); } catch (e) { res.push("E:" + (e as Error).name); }
  try { res.push(String(s.prod())); } catch (e) { res.push("E:" + (e as Error).name); }
}
console.log(res.join("|"), log.length);
