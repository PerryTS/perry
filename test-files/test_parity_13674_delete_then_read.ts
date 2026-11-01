import { bindDeep } from "./_helpers/parity_13674_delete_then_read_a.ts";
function leak(o: any): void { (globalThis as any).g = o; }
class Leaky {
  k = 5;
  lm = 7;
  get2(): number { leak(this); return this.k; }
}
class Plain {
  k = 5;
  lm = 7;
}
const lk = new Leaky();
class Getter { get host(): Leaky { return lk; } }
const g = bindDeep(new Getter());
const r = g();
delete (globalThis as any).g.k;
const kk = lk.k; console.log(r, lk.lm, kk, typeof kk, kk === undefined, kk + 1, "k" in lk);
