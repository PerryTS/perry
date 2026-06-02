// Gap test: TypeScript explicit `this: T` parameter annotation must be
// erased — it is a TYPE-only marker, never a runtime parameter. Without
// erasure, a method `m(this: T, fin)` is lowered as a 2-arg function and
// the real first argument lands in the `this` slot, so `fin` reads
// undefined. This is the effect Layer/Scope blocker (#321): every
// `ScopeImplProto` method uses `(this: ScopeImpl, fin)` and dispatches
// through `Object.create(proto)`.
// Run: node --experimental-strip-types test_gap_this_param_annotation.ts

// --- object-literal method with explicit `this` param, called directly ---
const litObj = {
  m(this: any, fin: any) {
    return typeof fin;
  },
};
console.log("literal direct:", litObj.m(42)); // number

// --- object-literal method reached via Object.create prototype chain ---
const Proto = {
  describe(this: any, label: string, n: number) {
    return label + ":" + n;
  },
};
const inst: any = Object.create(Proto);
console.log("Object.create proto:", inst.describe("count", 7)); // count:7

// --- a method body that reads `this` AND a forwarded arg ---
const Counter = {
  base: 100,
  add(this: any, delta: number) {
    return this.base + delta;
  },
};
const counter: any = Object.create(Counter);
console.log("this + arg:", counter.add(5)); // 105

// --- class method with explicit `this` param ---
class Greeter {
  prefix = "Hello, ";
  greet(this: Greeter, name: string): string {
    return this.prefix + name;
  }
}
const g = new Greeter();
console.log("class method:", g.greet("Ada")); // Hello, Ada

// --- multiple args after `this` ---
const Math2 = {
  combine(this: any, a: number, b: number, c: number) {
    return a * 100 + b * 10 + c;
  },
};
const m2: any = Object.create(Math2);
console.log("multi-arg:", m2.combine(1, 2, 3)); // 123

// --- plain function with `this` param + Function.prototype.call ---
function tag(this: any, suffix: string): string {
  return "tagged-" + suffix;
}
console.log("fn.call:", tag.call({}, "x")); // tagged-x
