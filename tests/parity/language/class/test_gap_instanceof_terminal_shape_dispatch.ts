// parity-node-argv: --expose-gc
"use strict";
export {};
declare function gc(): void;
const DateCtor: any = Date;
const MapCtor: any = Map;
const ObjectCtor: any = Object;
const terminal: any = {};
const nil: any = Object.create(null);
function check(value: any, ctor: any): boolean { return value instanceof ctor; }
for (let i = 0; i < 100; i++) {
  if (check(terminal, DateCtor) || check(nil, MapCtor)) throw new Error("terminal miss");
  if (!check(terminal, ObjectCtor) || check(nil, ObjectCtor)) throw new Error("terminal target");
}
console.log("terminal", check(terminal, DateCtor), check(nil, MapCtor), check(terminal, ObjectCtor));
const Ordinary: any = function Ordinary() {};
Ordinary.prototype = Object.prototype;
console.log("actual target", check(terminal, Ordinary), check(nil, Ordinary));
Ordinary.prototype = {};
console.log("changed target", check(terminal, Ordinary));
// Negative controls: a non-terminal receiver and a native cell must retain
// their positive answers rather than taking a terminal-shape miss.
const dateChild: any = Object.create(DateCtor.prototype);
console.log("native and linked", check(new DateCtor(0), DateCtor), check(dateChild, DateCtor));
// Weak collection shapes carry a native brand even when their link word is
// DEFAULT. That brand keeps them out of the ordinary terminal proof.
const WeakMapCtor: any = WeakMap;
const WeakSetCtor: any = WeakSet;
console.log("native brands", check(new WeakMapCtor(), WeakMapCtor), check(new WeakSetCtor(), WeakSetCtor));
Object.setPrototypeOf(terminal, MapCtor.prototype);
console.log("changed receiver", check(terminal, MapCtor), check(terminal, ObjectCtor));
// A bound constructor delegates even when its target's hook answers true
// for a receiver whose ordinary prototype chain is empty.
const Hook: any = function Hook() {};
Object.defineProperty(Hook, Symbol.hasInstance, { value(v: any) { return v === nil; } });
const Bound: any = Hook.bind(null);
console.log("bound hook", check(nil, Bound));
// A getter may change and move the receiver before ordinary dispatch resumes.
const moving: any = {};
let reads = 0;
Object.defineProperty(MapCtor, Symbol.hasInstance, {
  configurable: true,
  get() {
    reads++;
    Object.setPrototypeOf(moving, MapCtor.prototype);
    if (typeof gc === "function") gc();
    return undefined;
  }
});
console.log("hook mutation", check(moving, MapCtor), reads);
delete MapCtor[Symbol.hasInstance];
let threw = false;
try { check(nil, { prototype: Object.prototype }); } catch (e) { threw = e instanceof TypeError; }
console.log("noncallable", threw);
