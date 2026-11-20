// #10498: class getters/setters reached through an untyped receiver are served
// by a (class, shape, key) -> accessor cache once the generic path has resolved
// them. Every loop below runs long enough to hit that cache, and every
// mutation after a warm loop must be seen by the next access.

const log: string[] = [];

class Res {
  _points = 0;
  _ms = 0;
  sets = 0;
  constructor(a: number, b: number) {
    this.points = a;
    this.ms = b;
  }
  get points() { return this._points; }
  set points(p: number) { this._points = p; this.sets++; }
  get ms() { return this._ms; }
  set ms(v: number) { this._ms = v; this.sets++; }
}

// Constructor stores through the setters, then reads through the getters.
let sum = 0;
let sets = 0;
for (let i = 0; i < 2000; i++) {
  const r: any = new Res(i & 7, i & 1023);
  sum += r.points + r.ms;
  sets += r.sets;
}
console.log("ctor", sum, sets);

// Untyped writes and reads on one receiver.
const fixed: any = new Res(3, 4);
let acc = 0;
for (let i = 0; i < 2000; i++) {
  fixed.points = i & 7;
  fixed.ms = i & 3;
  acc += fixed.points * 10 + fixed._ms;
}
console.log("loop", acc, fixed.sets, fixed.points, fixed.ms);

// A subclass inheriting one accessor and overriding the other.
class Sub extends Res {
  get ms() { return this._ms * 100; }
  set ms(v: number) { this._ms = v + 1; }
}
const subs: any[] = [new Res(1, 2), new Sub(1, 2)];
let mixed = 0;
for (let i = 0; i < 1000; i++) {
  const o = subs[i & 1];
  o.ms = i & 3;
  mixed += o.ms + o.points;
}
console.log("sub", mixed, subs[0].ms, subs[1].ms);

// Receivers of many shapes reading one accessor at one site.
const shaped: any[] = [];
for (let i = 0; i < 8; i++) {
  const r: any = new Res(i, i);
  (r as any)["extra" + i] = i;
  shaped.push(r);
}
let shapeSum = 0;
for (let i = 0; i < 800; i++) shapeSum += shaped[i & 7].points;
console.log("shapes", shapeSum);

// Redefining the accessor on the prototype after the cache is warm: the next
// write must reach the new setter, not the cached one.
class Box {
  _v = 1;
  get v() { return this._v; }
  set v(x: number) { this._v = x; }
}
const box: any = new Box();
let before = 0;
for (let i = 0; i < 500; i++) { box.v = i; before += box.v; }
Object.defineProperty(Box.prototype, "v", {
  get() { return -1; },
  set(x: number) { log.push("redefined set " + x); },
  configurable: true,
});
box.v = 7;
console.log("redefine", before, box._v, log.join(","));

// An own property shadowing the accessor after the cache is warm.
class Shadow {
  _s = 1;
  get s() { return this._s; }
  set s(x: number) { this._s = x; }
}
const sh: any = new Shadow();
for (let i = 0; i < 500; i++) { sh.s = i; }
Object.defineProperty(sh, "s", { value: 99, writable: true, enumerable: true, configurable: true });
sh.s = 5;
const other: any = new Shadow();
other.s = 6;
console.log("shadow", sh.s, sh._s, other.s, other._s);

// Replacing an instance's prototype after the cache is warm.
class Proto {
  _p = 2;
  get p() { return this._p * 2; }
}
const pinst: any = new Proto();
let psum = 0;
for (let i = 0; i < 500; i++) psum += pinst.p;
Object.setPrototypeOf(pinst, { get p() { return 1000; } });
console.log("setproto", psum, pinst.p, new Proto().p);

// A getter that throws while its entry is warm keeps being served afterward.
class Thrower {
  n = 0;
  get t() {
    this.n++;
    if (this.n === 300) throw new Error("boom at " + this.n);
    return this.n;
  }
}
const th: any = new Thrower();
let caught = "";
let tsum = 0;
for (let i = 0; i < 600; i++) {
  try { tsum += th.t; } catch (e) { caught = (e as Error).message; }
}
console.log("throw", tsum, caught, th.n);

// Accessors named like keys other arms answer by name stay correct.
class Named {
  _len = 4;
  get length() { return this._len; }
  set length(v: number) { this._len = v * 2; }
  get size() { return 11; }
}
const named: any = new Named();
let nsum = 0;
for (let i = 0; i < 300; i++) { named.length = i; nsum += named.length + named.size; }
console.log("named", nsum, named.length);
