// Class families whose constructors run code between their stores: values
// that read the half-built instance, call its methods, bind them, store other
// keys out of order, or change the prototype chain before a later store.
function show(tag: string, o: any) {
  console.log(tag, JSON.stringify(Object.keys(o)), JSON.stringify(o));
}

let peek: string[] = [];
class Base {
  [k: string]: any;
  constructor(def: any) {
    this.first = 1;
    this.spa = this.later;
    this.def = def;
    this.parse = this.parse.bind(this);
    this.seen = observe(this);
    this.tail = def && def.tail;
  }
  parse(x: any) { return [x, this.def]; }
  later() { return "proto-later"; }
}
function observe(o: any) {
  peek.push(Object.keys(o).join(","));
  peek.push(String("tail" in o) + ":" + String(o.tail) + ":" + String(o.hasOwnProperty("seen")));
  return peek.length;
}
const b = new Base({ tail: "t" });
show("base", b);
console.log("peek", peek.join("|"));
console.log("parse", JSON.stringify(b.parse(3)), typeof b.spa);

// A value that stores a later key first: the enumeration order is the
// insertion order.
class Ooo {
  [k: string]: any;
  constructor() {
    this.a = 1;
    this.b = (this.d = "early", 2);
    this.c = touch(this);
    this.d = "late";
  }
}
function touch(o: any) { o.e = "extra"; return Object.keys(o).length; }
for (let i = 0; i < 3; i++) show("ooo" + i, new Ooo());

// A value that defines a setter for a later constructor key on the
// prototype: the later store must call it.
let arm = false;
class Trap {
  [k: string]: any;
  constructor() {
    this.x = 1;
    this.y = arm ? defineSetter() : 2;
    this.z = 3;
  }
}
function defineSetter() {
  Object.defineProperty(Trap.prototype, "z", {
    set(v: any) { console.log("setter z", v); },
    get() { return "getter z"; },
    configurable: true,
  });
  return 20;
}
show("trap0", new Trap());
arm = true;
const t1: any = new Trap();
show("trap1", t1);
console.log("z", t1.z, Object.prototype.hasOwnProperty.call(t1, "z"));
delete (Trap.prototype as any).z;
show("trap2", new Trap());

// A derived class whose super(...) arguments run code that freezes the
// prototype path of a parent key.
class P {
  [k: string]: any;
  constructor(a: any) { this.p = a; this.q = 2; }
}
let ro = false;
class C extends P {
  constructor(...args: any[]) {
    super(ro ? makeReadonly() : args[0]);
    this.r = 3;
  }
}
function makeReadonly() {
  Object.defineProperty(P.prototype, "q", { value: "ro", writable: false, configurable: true });
  return "x";
}
show("c0", new C(1));
ro = true;
try { show("c1", new C(1)); } catch (e: any) { console.log("c1 throws", e.constructor.name); }
delete (P.prototype as any).q;
ro = false;
show("c2", new C(5));

// A constructor that throws half-way: the escaped instance holds only the
// keys stored before.
let escaped: any = null;
class Half {
  [k: string]: any;
  constructor(fail: boolean) {
    this.u = 1;
    this.v = grab(this, fail);
    this.w = 3;
  }
  sum() { return [this.u, this.v, this.w, "w" in this]; }
}
function grab(o: any, fail: boolean) { escaped = o; if (fail) throw new Error("x"); return 2; }
try { new Half(true); } catch (e) { console.log("half threw"); }
show("half", escaped);
console.log("half sum", JSON.stringify(escaped.sum()));
escaped.w = 9;
show("half after", escaped);
show("whole", new Half(false));

// Many births, mixed paths.
let total = 0;
for (let i = 0; i < 300; i++) {
  const o: any = i % 2 ? new Base({ tail: i }) : new Ooo();
  total += Object.keys(o).length + (o.tail === i ? 1 : 0);
}
console.log("total", total);
