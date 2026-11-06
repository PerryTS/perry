// WPS3 class B: `extends <expression>` whose value is one class P. The parent's field
// initializers run before its constructor body and before the subclass fields, in one order.
class P {
  p1 = 1;
  seen: string;
  constructor() { this.seen = Object.keys(this).join("|"); }
}
function mk(): typeof P { return P; }
class D extends (mk() as any) { g1 = "g1"; }
class E extends (mk() as any) { constructor() { super(); (this as any).e1 = "e1"; } }
function show(o: any) { console.log(Object.keys(o).join(","), o.p1, o.seen, o.g1, o.e1); }
show(new D());
show(new E());
