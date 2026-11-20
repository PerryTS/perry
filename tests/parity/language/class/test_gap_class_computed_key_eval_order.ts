// THR1: computed keys run in source order, interleaved with computed field
// keys; static field initialisers and static blocks run after every key, in
// order; instance field initialisers run per construction.
const log: string[] = [];
function k(name: string): string { log.push("key:" + name); return name; }
function v(name: string): number { log.push("init:" + name); return name.length; }
class F {
  static [k("s1")] = v("s1");
  [k("m1")]() { return 1; }
  *[Symbol.iterator]() { yield 1; }
  [k("f1")] = v("f1");
  static { log.push("block"); }
  get [k("g1")]() { return 2; }
  static [k("s2")] = v("s22");
  [k("m2")]() { return 3; }
}
console.log(log.join(" "));
log.length = 0;
const f: any = new F();
console.log(log.join(" "), (F as any).s1, (F as any).s2, f.f1, f.m1(), f.g1, f.m2());
console.log(Reflect.ownKeys(F.prototype).map(String).join(","));
