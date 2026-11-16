// KEEP: a subclass setter for the key: the base-method store must run the setter
class Obj { flag: boolean; radius: number; constructor() { this.flag = false; this.radius = 1; }
  world() { this.radius = this.radius * 2; this.flag = true; } }
class Grp extends Obj { log: number[] = []; set flag2(v: boolean) { this.log.push(v ? 1 : 0); } }
class Acc extends Obj { _r = 0; get radius(): number { return this._r; } set radius(v: number) { this._r = v + 1; } }
class Sph { radius: number; constructor(r: number) { this.radius = r; } }
function rr(o: Obj | Sph): number { return o.radius; }
const k = Number(process.argv[2] || "2");
const objs: Obj[] = [new Obj(), new Grp(), new Acc()];
let t = 0;
for (let i = 0; i < 5; i++) { for (const o of objs) { o.world(); t += rr(o); } t += rr(new Sph(k)); }
console.log("RESULT", t, objs[2].radius, (objs[2] as Acc)._r);
