// Class family constructors whose stores form runs (object model step 2c):
// one check covers the stores between two points where user code can run.
// Each case changes what a later store in the run must do from inside a value
// the run continues across, or right before the run starts.
function show(tag: string, o: any) {
  console.log(tag, JSON.stringify(Object.keys(o)), JSON.stringify(o));
}

// A run across `this.m.bind(this)`: a patched Function.prototype.bind is user
// code, so the store after it must call the setter it defines.
let patchBind = false;
const realBind = Function.prototype.bind;
class Bound {
  [k: string]: any;
  constructor(x: any) {
    this.s = this.k0;
    this.a = x;
    this.m = this.m.bind(this);
    this.z = 3;
  }
  get k0() { return "k0"; }
  m() { return this.a; }
}
show("bound0", new Bound(1));
patchBind = true;
(Function.prototype as any).bind = function (this: any, ...args: any[]) {
  if (patchBind) {
    patchBind = false;
    Object.defineProperty(Bound.prototype, "z", {
      set(v: any) { console.log("setter z", v); },
      get() { return "getter z"; },
      configurable: true,
    });
  }
  return realBind.apply(this, args as any);
};
const b1: any = new Bound(2);
show("bound1", b1);
console.log("z", b1.z, Object.prototype.hasOwnProperty.call(b1, "z"));
(Function.prototype as any).bind = realBind;
delete (Bound.prototype as any).z;
show("bound2", new Bound(3));

// A method with its own `bind`: the call is user code.
class OwnBind {
  [k: string]: any;
  constructor() {
    this.s = this.k0;
    this.m = this.m.bind(this);
    this.y = 2;
  }
  get k0() { return 0; }
  m() { return 1; }
}
(OwnBind.prototype.m as any).bind = function () {
  Object.defineProperty(OwnBind.prototype, "y", {
    set(v: any) { console.log("setter y", v); },
    configurable: true,
  });
  return () => 7;
};
show("ownbind", new OwnBind());
delete (OwnBind.prototype as any).y;
show("ownbind2", new OwnBind());

// A value that makes the instance non-extensible, or moves its prototype, in
// the middle of the constructor.
let mode = 0;
const Alt: any = { set w(v: any) { console.log("alt setter w", v); } };
class Mid {
  [k: string]: any;
  constructor() {
    this.u = 1;
    this.v = change(this);
    this.w = 3;
    this.x = this.u;
  }
}
function change(o: any) {
  if (mode === 1) Object.preventExtensions(o);
  if (mode === 2) Object.setPrototypeOf(o, Alt);
  if (mode === 3) Object.create(o);
  return mode;
}
for (mode = 0; mode < 4; mode++) {
  try { show("mid" + mode, new Mid()); } catch (e: any) { console.log("mid" + mode + " throws", e.constructor.name); }
}

// A key stored twice in one run, and a run that reads keys it has stored.
class Twice {
  [k: string]: any;
  constructor(d: any) {
    this.spa = this.k1;
    this.def = d;
    this.k1 = this.k1.bind(this);
    this.spa = this.spa.bind(this);
    this.copy = this.def;
    this.obj = { d: this.def, f: () => this.def };
  }
  k1() { return this.def; }
}
const tw: any = new Twice("D");
show("twice", tw);
console.log("twice calls", tw.k1(), tw.spa(), tw.obj.f());

// A method that deletes a constructor key and stores it again: the key is
// last in the order.
class Readd {
  [k: string]: any;
  constructor(a: any) { this.a = a; this.b = 2; this.c = 3; }
  again() { delete this.a; this.a = "again"; return this; }
}
show("readd", new Readd(1).again());

let total = 0;
for (let i = 0; i < 200; i++) {
  const o: any = i % 3 === 0 ? new Twice(i) : i % 3 === 1 ? new Bound(i) : new Mid();
  total += Object.keys(o).length;
}
console.log("total", total);
