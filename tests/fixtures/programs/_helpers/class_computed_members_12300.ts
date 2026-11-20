// Shared by test_gap_12300_worker_class_computed_members.ts and
// test_issue_12300_thread_class_computed_members.ts: a class whose instance
// members are computed (string, well-known symbol, registered symbol, unique
// symbol, accessor) and interleaved with literal ones, so a realm that never
// named a computed member would lose every member declared after it.
const k = "comp" + "uted";
const sym = Symbol.for("effect/Hash");
const own = Symbol("own");
export class C {
  v = 3;
  [k]() { return "c" + this.v; }
  later() { return "l" + this.v; }
  *[Symbol.iterator]() { yield 1; yield this.v; }
  [sym]() { return "h" + this.v; }
  [own]() { return "o" + this.v; }
  get [k + "G"]() { return this.v * 2; }
  last() { return "z" + this.v; }
  static [k + "S"]() { return "s"; }
}
export function exercise(o: any): string {
  const proto = Object.getPrototypeOf(o);
  const syms = Object.getOwnPropertySymbols(proto);
  const r: any[] = [
    o.computed(),
    o.later(),
    [...o],
    o[Symbol.for("effect/Hash")](),
    o.computedG,
    o.last(),
    (o.constructor as any).computedS(),
    Object.getOwnPropertyNames(proto).join(","),
    syms.map((s) => s.description).join(","),
    o[syms[2]](),
  ];
  return JSON.stringify(r);
}
