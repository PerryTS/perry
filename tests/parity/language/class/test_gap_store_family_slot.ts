// RED: a family colouring moves a constructor key; stores in base methods called on subclasses
class Obj { id: number; flag: boolean; radius: number;
  constructor() { this.id = 1; this.flag = false; this.radius = 1; }
  upd() { this.flag = true; }
  world() { this.upd(); this.radius = this.radius * 1.5; this.flag = false; } }
class Grp extends Obj { kids: number; constructor() { super(); this.kids = 0; } }
class Msh extends Obj { geo: string; constructor() { super(); this.geo = "g"; } }
class Sph { radius: number; constructor(r: number) { this.radius = r; }
  copy(s: Sph) { this.radius = s.radius; return this; } }
function rr(o: Obj | Sph): number { return o.radius; }
const k = String(Math.random()).length > 0 ? Number(process.argv[2] || "2") : 0;
const objs: Obj[] = [new Obj(), new Grp(), new Msh()];
const a = new Sph(k), b = new Sph(0);
let t = 0;
for (let i = 0; i < 20; i++) { for (const o of objs) { o.world(); o.radius = o.radius % 7; t += rr(o); } b.copy(a); t += rr(b); }
console.log("RESULT", t, String(objs[1].flag), objs[2].radius);
