// Element operands the facts do NOT prove Number-or-undefined keep the
// generic path: a String element, a holey literal, BigInt elements, an
// object with valueOf (call order), `-0`, comparisons with NaN / undefined.
const arg = process.argv[2] ?? String(Math.floor(Math.random() * 0.5));
const log: string[] = [];
class K {
  strs: any[]; holes: any[]; bigs: any[]; objs: any[];
  constructor() {
    this.strs = [arg, 2, 3];
    this.holes = [1, , 3];
    this.bigs = [BigInt(arg.length), 2n, 3n];
    this.objs = [{ valueOf() { log.push("a"); return 2; } }, { valueOf() { log.push("b"); return -0; } }, 4];
  }
  str(): any { const t = this.strs; return t[1] * t[2] + t[0]; }
  hole(): any { const t = this.holes; return t[0] * t[1] + t[2]; }
  big(): any { const t = this.bigs; return t[0] * t[1] + t[2]; }
  obj(): any { const t = this.objs; return t[0] * t[1] + t[2] * t[1]; }
  cmp(): string { const t = this.holes; return [t[1] < t[0], t[1] >= t[0], t[1] === t[1], t[0] * t[1] < 1, NaN <= t[0]].join(","); }
}
const k = new K();
const o = k.obj();
console.log(k.str(), k.hole(), String(k.big()), o, Object.is(o, -0), log.join(""), k.cmp());
