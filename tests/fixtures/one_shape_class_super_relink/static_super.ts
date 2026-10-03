// A program with no prototype surgery: `super.name` in a static member names
// the parent CONSTRUCTOR's member, which the compiler must not resolve from
// the parent's INSTANCE method tables, for calls, `typeof super.m` and
// `super.m` as a value. (A program that relinks or patches a prototype sends
// these through the runtime lookup, so one_shape_class_super_relink cannot
// see the compile-time route.) Expected output is node 26.5.1's.
function show(name: string, v: any): void {
  console.log(name + " " + String(v));
}
function tryIt(f: () => any): string {
  try {
    return "ok:" + String(f());
  } catch (e) {
    return (e instanceof TypeError) ? "TypeError" : "other:" + String(e);
  }
}
class StB {
  onlyInst() { return "StB.inst-only"; }
  static both() { return "StB.static-both:" + (this as any).name; }
  both() { return "StB.inst-both"; }
  static onlyStatic() { return "StB.static-only:" + (this as any).name; }
}
class StC extends StB {
  static callOnlyInst() { return super.onlyInst(); }
  static callBoth() { return super.both(); }
  static callOnlyStatic() { return super.onlyStatic(); }
  static typeOnlyInst() { return typeof super.onlyInst; }
  static typeBoth() { return typeof super.both; }
  static valueOnlyInst() { return super.onlyInst; }
  static valueBoth() { return super.both === StB.both; }
  callBothInst() { return super.both(); }
  typeOnlyStatic() { return typeof super.onlyStatic; }
}
show("call-inst-only", tryIt(() => StC.callOnlyInst()));
show("call-both", tryIt(() => StC.callBoth()));
show("call-only-static", tryIt(() => StC.callOnlyStatic()));
show("typeof-inst-only", StC.typeOnlyInst());
show("typeof-both", StC.typeBoth());
show("value-inst-only", StC.valueOnlyInst());
show("value-both", StC.valueBoth());
show("instance-side", new StC().callBothInst());
show("instance-typeof-static-only", new StC().typeOnlyStatic());
