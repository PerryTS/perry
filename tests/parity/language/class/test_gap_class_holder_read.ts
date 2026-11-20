// Inherited method reads through a class family's layout (object model 3.6):
// `this.m` in an inlined constructor and `o.m` on a `new C()` binding read
// the holder's slot while every prototype is on its static shape, and take
// the ordinary read whenever the chain or the receiver changed.

class Base {
  declare tag: string;
  declare m: any;
  declare n: any;
  constructor(tag: string) {
    this.tag = tag;
    this.m = this.m.bind(this);
    this.n = typeof this.n === "function" ? this.n() : "no-n";
  }
  m() { return "Base.m:" + this.tag; }
  n() { return "Base.n"; }
  p() { return "Base.p"; }
}
class Mid extends Base {}
class Leaf extends Mid {
  p() { return "Leaf.p"; }
}
class Over extends Mid {
  m() { return "Over.m:" + this.tag; }
}

function show(label: string, v: any) {
  console.log(label, typeof v === "function" ? v() : v);
}

const a = new Leaf("a");
show("a.m", a.m);
show("a.n", a.n);
show("a.p", a.p);
const o = new Over("o");
show("o.m", o.m);

// A method replaced on the holder after the first births.
(Base.prototype as any).n = function () { return "Base.n2"; };
const b = new Leaf("b");
show("b.n", b.n);

// A prototype between gains an own method.
(Mid.prototype as any).p = function () { return "Mid.p"; };
const c = new Mid("c");
show("c.p", c.p);
show("a.p again", a.p);

// An accessor on an intermediate prototype.
Object.defineProperty(Mid.prototype, "n", { get() { return function () { return "Mid.get n"; }; }, set(v: any) { (this as any)._n = v; }, configurable: true });
const d = new Leaf("d");
show("d.n", d.n);

// Deleted from the holder: the read continues the chain.
delete (Base.prototype as any).p;
const e = new Mid("e");
show("e.p", e.p);

// An instance relinked to another prototype.
const f = new Leaf("f");
Object.setPrototypeOf(f, { p() { return "relinked.p"; } });
show("f.p", (f as any).p);

// An own key written before the read.
class Own {
  declare q: any;
  constructor() {
    this.q = function () { return "own q"; };
    show("own q", this.q);
  }
  q2() { return 1; }
}
new Own();
console.log("done");
