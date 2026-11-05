// Regression: `super.<method>(event, ...rest)` forwarding a rest parameter
// to a NATIVE base method (EventEmitter.prototype.emit) must spread the rest
// elements as individual arguments — not deliver the rest array as one arg.
//
// Two latent codegen bugs were involved:
//   1. The own-method-override runtime check passed the STATIC ABI's
//      rest-bundled args to the dynamic-override branch (`js_native_call_value`),
//      so the native override received `[event, [payload]]` and listeners saw
//      `[payload]` instead of `payload`.
//   2. `SuperMethodCall` dropped the spread marker, so the static dispatch path
//      passed the rest array as a single positional argument.
import { EventEmitter as EE } from "events";

let M: any;
M = class M extends EE {
  constructor() {
    super();
  }
  emit(event: any, ...args: any[]) {
    return super.emit(event, ...args);
  }
};

const m: any = new M();

let one: any = null;
m.on("one", (p: string) => {
  one = p;
});
m.emit("one", "PAYLOAD");
console.log("one:", JSON.stringify(one)); // expect "PAYLOAD"

let a: any = null;
let b: any = null;
m.on("two", (x: string, y: string) => {
  a = x;
  b = y;
});
m.emit("two", "A", "B");
console.log("two:", JSON.stringify(a), JSON.stringify(b)); // expect "A" "B"

let zero = "no";
m.on("zero", () => {
  zero = "yes";
});
m.emit("zero");
console.log("zero:", zero); // expect yes

// Spread forwarding through a fixed-arity JS parent (static dispatch path).
class Base {
  recv(x: any, y: any) {
    return JSON.stringify(x) + "|" + JSON.stringify(y);
  }
}
class Child extends Base {
  recv(...rest: any[]) {
    return super.recv(...(rest as [any, any]));
  }
}
console.log("fixed-arity:", new Child().recv("X", "Y")); // expect "X"|"Y"

// Instance-level override of a rest-param method must receive flat args.
class Collector {
  collect(...items: any[]) {
    return "orig:" + items.length;
  }
}
const c: any = new Collector();
c.collect = (...items: any[]) => "ovr:" + items.length;
console.log("override-rest:", c.collect("a", "b", "c")); // expect ovr:3
