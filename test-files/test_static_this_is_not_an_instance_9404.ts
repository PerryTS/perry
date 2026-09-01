// #9404: inside a STATIC body `this` is the CONSTRUCTOR object, not an
// instance. The compiler must not type it as an instance of the class:
// instance members must be absent, and static members must still resolve.
class P {
  static sf = 7;
  static sobj = { a: 1 };
  x = 3;
  m() { return 1; }
  static probeMethod() { return typeof this.m; }
  static probeField() { return typeof this.x; }
  static probeValue() { return String((this as any).m); }
  // NOT asserted: the CALL form `this.m()` with a static-`this` receiver.
  // The VALUE read is fixed (see `probeValue` above — `undefined`), and both
  // `P.m()` on the class binding and read-then-call (`const f = this.m; f()`)
  // throw correctly, but the runtime call tower still resolves the instance
  // vtable method for a CONSTRUCTOR class ref given a method name. It is keyed
  // on the runtime class id — `class S extends B {}` calling an inherited
  // `static go(){ return this.m(); }` runs `S`'s override, not `B`'s — so it is
  // the runtime dispatch, not a codegen direct call. Repro:
  //   class B { m(){return "B.m"} static go(){ return (this as any).m() } }
  //   B.go()  // node: TypeError   perry: "B.m"
  static readStatic() { return this.sf; }
  static readStaticNested() { return this.sobj.a; }
  static isConstructor() { return this === P; }
  static other() { return 42; }
  static viaThis() { return this.other(); }
  static protoKind() { return typeof this.prototype; }
  static ownName() { return this.name; }
  static { console.log("static-block:", typeof this.m, this === P, this.sf); }
  static get sgetter() { return typeof this.m; }
}
console.log("typeof this.m:", P.probeMethod());
console.log("typeof this.x:", P.probeField());
console.log("String(this.m):", P.probeValue());
console.log("this.sf:", P.readStatic());
console.log("this.sobj.a:", P.readStaticNested());
console.log("this === P:", P.isConstructor());
console.log("this.other():", P.viaThis());
console.log("typeof this.prototype:", P.protoKind());
console.log("this.name:", P.ownName());
console.log("static getter:", P.sgetter);

// #9386 residual: aliasing `this` to a local made the receiver a LocalGet,
// so the static-`this` gate no longer applied and the wrong type reached
// the computed-member route through `local_types`.
const K = "dy" + "n";
class G {
  static sf = 9;
  [K]() { return 4; }
  static viaLocal() { const t: any = this; return [typeof t.prototype, t.sf].join("|"); }
  static aliasMethod() { const t: any = this; return typeof t.dyn; }
}
console.log("alias prototype|sf:", G.viaLocal());
console.log("alias computed method:", G.aliasMethod());

// A static body on a SUBCLASS: `this` is the SUBclass, not the declarer.
class Base { static tag = "base"; static who() { return this.name; } static readTag() { return this.tag; } }
class Kid extends Base { static tag = "kid"; }
console.log("subclass this.name:", Kid.who(), Base.who());
console.log("subclass this.tag:", Kid.readTag(), Base.readTag());

// A static method reaching another static method through `this` on a
// subclass resolves against the SUBCLASS's override.
class B3 { static a() { return "B3.a"; } static callA() { return this.a(); } }
class S3 extends B3 { static a() { return "S3.a"; } }
console.log("subclass static dispatch:", S3.callA(), B3.callA());

// An instance method of the same class is unaffected: `this` IS an instance.
class Inst { v = 5; get2() { return this.v * 2; } run() { return typeof this.get2; } }
console.log("instance side:", new Inst().get2(), new Inst().run());

// A static method whose name collides with a String method, called through
// `this` — the static receiver must dispatch to the static member.
class Collide {
  static split() { return "static-split"; }
  static go() { return (this as any).split(); }
}
console.log("collide:", Collide.go());

// The constructor-side half, with no `this` at all: a class object does not
// expose its prototype methods as statics. Reachable through every read form.
class R { m() { return 1; } static s() { return 2; } }
const key = "m";
console.log("C.m:", typeof (R as any).m, typeof (R as any)[key], typeof Reflect.get(R as any, "m"));
console.log("C.s:", typeof R.s);
console.log("C.prototype.m:", typeof R.prototype.m);
console.log("m in C:", "m" in (R as any), "| own:", Object.getOwnPropertyNames(R).join(","));
console.log("C.m === C.prototype.m:", (R as any).m === (R as any).prototype.m);

// A subclass must not inherit its parent's prototype methods as statics
// either, while genuine static inheritance keeps working.
class RSub extends R { static t() { return 3; } }
console.log("sub:", typeof (RSub as any).m, typeof RSub.s, typeof RSub.t);
