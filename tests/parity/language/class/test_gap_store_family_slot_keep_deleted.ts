// KEEP: the key is deleted in one subclass; the later store re-adds it
class Obj { flag: boolean; radius: number; constructor() { this.flag = false; this.radius = 1; }
  world() { this.radius = 3; this.flag = true; } }
class Del extends Obj { constructor() { super(); delete (this as any).radius; } }
class Sph { radius: number; constructor(r: number) { this.radius = r; } }
function rr(o: Obj | Sph): number { return o.radius; }
const k = Number(process.argv[2] || "2");
const objs: Obj[] = [new Obj(), new Del()];
let t = 0;
const before = Object.keys(objs[1]).join(",");
for (let i = 0; i < 5; i++) { for (const o of objs) { o.world(); t += rr(o); } t += rr(new Sph(k)); }
console.log("RESULT", t, before, Object.keys(objs[1]).join(","), objs[1].radius);
