// KEEP: Object.freeze(this) in one subclass: the store must throw (strict)
class Obj { flag: boolean; radius: number; constructor() { this.flag = false; this.radius = 1; }
  world() { this.radius = this.radius * 2; this.flag = true; } }
class Frz extends Obj { constructor() { super(); Object.freeze(this); } }
class Sph { radius: number; constructor(r: number) { this.radius = r; } }
function rr(o: Obj | Sph): number { return o.radius; }
const k = Number(process.argv[2] || "2");
const objs: Obj[] = [new Obj(), new Frz()];
let t = 0, e = 0;
for (let i = 0; i < 5; i++) { for (const o of objs) { try { o.world(); } catch (x) { e++; } t += rr(o); } t += rr(new Sph(k)); }
console.log("RESULT", t, e, objs[1].radius, String(objs[1].flag));
