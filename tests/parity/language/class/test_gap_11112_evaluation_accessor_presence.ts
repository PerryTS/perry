// #11112: virtual ClassBody accessors count as properties on evaluated prototypes.
let calls = 0;
function make(n: number) {
  class C {
    get value() { calls++; return n; }
    set writeOnly(v: number) { calls++; n = v; }
    method() { return n; }
  }
  return C;
}
const C = make(1);
const D = make(2);
const c: any = new C();
const d: any = new D();
console.log("captured", "value" in c, "writeOnly" in c, "method" in c);
console.log("own", Object.hasOwn(c, "value"));
console.log("prototype", "value" in C.prototype, "writeOnly" in C.prototype);
console.log("evaluations", "value" in d, C.prototype === D.prototype);
const inherited = Object.create(C.prototype);
console.log("created", "value" in inherited, "writeOnly" in inherited);
class Child extends C {}
const child: any = new Child();
console.log("subclass", "value" in child, "writeOnly" in child);
console.log("calls", calls);
console.log("read", c.value, d.value);
Object.setPrototypeOf(c, null);
console.log("null prototype", "value" in c, "writeOnly" in c);
Object.setPrototypeOf(c, { replacement: true });
console.log("replaced prototype", "value" in c, "replacement" in c);
Object.setPrototypeOf(c, C.prototype);
console.log("restored prototype", "value" in c);
delete (C.prototype as any).value;
console.log("deleted", "value" in c, "value" in inherited);
Object.defineProperty(C.prototype, "value", { get() { calls++; return 8; }, configurable: true });
console.log("redefined", "value" in c, "value" in inherited, calls);
class Plain { get value() { calls++; return 4; } }
console.log("plain", "value" in new Plain(), calls);
