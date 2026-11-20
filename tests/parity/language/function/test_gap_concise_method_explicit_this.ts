// An object-literal concise method (and accessor / symbol method) is lowered
// with `captures_this` and its reserved capture slot baked to the defining
// object at construction time, so its body reads `this` from that slot rather
// than IMPLICIT_THIS. The explicit-`this` entry points — Function.prototype
// `call`/`apply`/`bind`, Reflect.apply, and the bound-function trampoline —
// must rebind that baked slot so an explicit receiver is honored; previously
// they only set IMPLICIT_THIS and the baked slot won, so the explicit `this`
// was silently ignored (`proto.m.call({v:7})` read the prototype's `v`).
//
// This is the idiom schema/validation `$constructor` factories rely on:
// installing own bound methods over the prototype's keys
// (`inst[k] = proto[k].bind(inst)`) and `this.m = this.m.bind(this)` in a base
// constructor. Arrow functions must still IGNORE an explicit receiver (lexical
// `this`), and a bound function ignores a later re-binding.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

const proto: any = { m() { return (this as any).v; } };

// call / apply / bind with an explicit, different `this`
console.log(proto.m.call({ v: 7 }));            // 7
console.log(proto.m.apply({ v: 9 }, []));       // 9
console.log(proto.m.bind({ v: 42 })());         // 42

// own bound method shadowing the inherited prototype method
const inst: any = Object.create(proto);
Object.defineProperty(inst, "v", { value: 1, enumerable: false });
inst.m = proto.m.bind(inst);
console.log(inst.m());                           // 1

// an arrow keeps its lexical `this` regardless of call/bind
const mk: any = { v: "outer", f() { return () => (this as any).v; } };
const arrow = mk.f();
console.log(arrow.call({ v: "INNER" }));         // outer
console.log(arrow.bind({ v: "INNER" })());       // outer

// a regular function honors call's `this`
function g(this: any) { return this.v; }
console.log(g.call({ v: 5 }));                   // 5

// a bound function ignores a later call's `this`
const b = (function (this: any) { return this.v; }).bind({ v: 100 });
console.log(b.call({ v: 200 }));                 // 100

// partial-args bind preserved alongside the rebound `this`
const adder: any = { base: 10, add(x: number, y: number) { return (this as any).base + x + y; } };
console.log(adder.add.bind({ base: 100 }, 1)(2)); // 103

// Reflect.apply on a concise method honors the explicit receiver
console.log(Reflect.apply(proto.m, { v: 314 }, [])); // 314

// accessors and symbol methods use the same baked-`this` capture shape, so the
// explicit-receiver paths must rebind them too
const sym = Symbol("m");
const mixed: any = {
  get value() { return (this as any).v; },
  [sym]() { return (this as any).v; },
};
const getter = Object.getOwnPropertyDescriptor(mixed, "value")!.get as any;
console.log(getter.call({ v: 8 }));                 // 8
console.log(mixed[sym].call({ v: 11 }));            // 11
console.log(Reflect.apply(getter, { v: 21 }, []));  // 21
