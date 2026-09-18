// #10556: `new EventEmitter() instanceof EventEmitter` segfaulted. A native
// EventEmitter instance is a POINTER_TAG registry handle (a small id such as
// `0x38000`), and the `instanceof EventEmitter` brand check first asked two
// "is this a namespace / cluster worker object?" probes that only rejected
// addresses below `0x10000` before reading a GC header / class id.
import { EventEmitter } from "node:events";
import EE from "node:events";
import { inherits } from "node:util";

const rt = <T>(v: T): T => JSON.parse(JSON.stringify(v));

const e = new EventEmitter();
console.log("named:", e instanceof EventEmitter);
console.log("default:", new EE() instanceof EE);
console.log("mixed:", new EE() instanceof EventEmitter, e instanceof EE);
const Ctor: any = [EventEmitter][rt(0)];
console.log("dynamic:", e instanceof Ctor);
console.log("reflective:", (Function.prototype as any)[Symbol.hasInstance].call(EventEmitter, e));

// Non-emitters of every kind answer false without crashing.
const others: [string, any][] = [
  ["sso", rt("uri")],
  ["heap string", rt("x".repeat(30))],
  ["number", rt(3)],
  ["null", rt(null)],
  ["object", {}],
  ["array", [1, 2, 3]],
  ["map", new Map()],
  ["function", () => 1],
];
for (const [name, v] of others) console.log(`${name} instanceof EventEmitter:`, v instanceof EventEmitter);

// The emitter still works after the checks.
let fired = 0;
e.on("ping", (n: number) => (fired += n));
e.emit("ping", 2);
e.emit("ping", 3);
console.log("fired:", fired, "listeners:", e.listenerCount("ping"));

// #10556 subclass shape: `class Sub extends EventEmitter {}` compiled
// through `js_instanceof_dynamic`, which never registered/consulted the
// class-chain parent edge that Array/Map/Set/Error subclassing uses — so a
// genuine subclass instance (a real ObjectHeader carrying Sub's own class
// id, not a handle, not prototype-linked to the real
// `EventEmitter.prototype`) never matched. Covers: direct subclass
// instanceof, the subclass's own constructor, a two-level subclass, the
// default-import form, and a util.inherits-style function-constructor
// subclass (prototype-chain linking, not a class `extends` edge — a
// different code path from the class-chain parent edge above).
class Sub extends EventEmitter {}
class Sub2 extends Sub {}
class SubDefault extends EE {}

const s = new Sub();
console.log("sub instanceof EventEmitter:", s instanceof EventEmitter);
console.log("sub instanceof Sub:", s instanceof Sub);

const s2 = new Sub2();
console.log("sub2 instanceof EventEmitter:", s2 instanceof EventEmitter);
console.log("sub2 instanceof Sub:", s2 instanceof Sub);
console.log("sub2 instanceof Sub2:", s2 instanceof Sub2);

const sd = new SubDefault();
console.log("subDefault instanceof EE:", sd instanceof EE);
console.log("subDefault instanceof EventEmitter:", sd instanceof EventEmitter);

// A subclass instance still behaves like a real emitter.
let subFired = 0;
s.on("ping", (n: number) => (subFired += n));
s.emit("ping", 5);
console.log("sub fired:", subFired, "listeners:", s.listenerCount("ping"));

// util.inherits-style function-constructor subclass: links prototypes at
// runtime rather than creating an `extends` edge, so it exercises the
// ordinary-prototype-walk fallback instead of the class-chain parent edge.
function FnEmitter(this: any) {
  EventEmitter.call(this);
}
inherits(FnEmitter, EventEmitter);
const fe: any = new (FnEmitter as any)();
console.log("fnEmitter instanceof EventEmitter:", fe instanceof EventEmitter);
