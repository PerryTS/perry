// S8: family-born instances define their fields with DefineField semantics
// (no inherited setter), fill their slots in a compact birth, and keep key
// order and presence under every receiver the constructor can meet.
const out: string[] = [];
const keys = (o: object) => Object.keys(o).join(",");

class Link {
  readonly to: any;
  readonly transformation: any;
  constructor(to: any, transformation: any) {
    this.to = to;
    this.transformation = transformation;
  }
}
out.push("link " + keys(new Link(1, 2)) + " " + JSON.stringify(new Link(1, 2)));
out.push("short " + keys(new (Link as any)(1)) + " " + JSON.stringify(new (Link as any)(1)));

class Link2 extends Link {
  z = 3;
}
out.push("link2 " + keys(new Link2(1, 2)) + " " + JSON.stringify(new Link2(1, 2)));

class Other {
  marker() { return "other"; }
}
const rc: any = Reflect.construct(Link, [5, 6], Other);
out.push("reflect " + keys(rc) + " " + (Object.getPrototypeOf(rc) === Other.prototype) + " " + rc.marker());

// A setter on Object.prototype: DefineField must not call it; a
// constructor's assignment of a key that is not a field must.
let setterCalls = 0;
Object.defineProperty(Object.prototype, "viaSetter", {
  set(_v: any) { setterCalls++; },
  get() { return "proto-get"; },
  configurable: true,
});
class FieldOverSetter {
  viaSetter = 1;
  other = 2;
}
const fs = new FieldOverSetter();
out.push("field-over-setter " + keys(fs) + " own=" + Object.prototype.hasOwnProperty.call(fs, "viaSetter") + " calls=" + setterCalls);
class StoreOverSetter {
  a = 1;
  constructor() { (this as any).viaSetter = 9; }
}
const ss = new StoreOverSetter();
out.push("store-over-setter " + keys(ss) + " own=" + Object.prototype.hasOwnProperty.call(ss, "viaSetter") + " calls=" + setterCalls);
delete (Object.prototype as any).viaSetter;

// A parent constructor that makes the receiver non-extensible.
class Sealer {
  constructor() { Object.preventExtensions(this); }
}
class AfterSeal extends Sealer {
  x = 1;
}
try { new AfterSeal(); out.push("after-seal no throw"); } catch (e: any) { out.push("after-seal " + e.constructor.name); }

// Methods and fields: the prototype holder is built by a field definition.
class WithMethod {
  a = 1;
  b: any;
  constructor(b: any) { this.b = b; }
  sum() { return this.a + this.b; }
}
let total = 0;
for (let i = 0; i < 1000; i++) total += new WithMethod(i).sum();
out.push("with-method " + total + " " + keys(new WithMethod(0)));

// this escaping from a field initializer before later fields.
const seen: string[] = [];
class Escapes {
  a = 1;
  b = ((self: any) => { seen.push(keys(self)); self.late = 0; return 2; })(this);
  c = 3;
}
out.push("escapes " + keys(new Escapes()) + " seen=" + seen.join("|"));

// Many instances, conditionally extended afterwards.
const many: any[] = [];
for (let i = 0; i < 500; i++) { const l: any = new Link(i, -i); if (i % 3 == 0) l.extra = i; many.push(l); }
out.push("many " + Array.from(new Set(many.map(keys))).join(" | ") + " " + many.reduce((s, l) => s + l.to + l.transformation, 0));
console.log(out.join("\n"));
