// #10556: `new EventEmitter() instanceof EventEmitter` segfaulted. A native
// EventEmitter instance is a POINTER_TAG registry handle (a small id such as
// `0x38000`), and the `instanceof EventEmitter` brand check first asked two
// "is this a namespace / cluster worker object?" probes that only rejected
// addresses below `0x10000` before reading a GC header / class id.
import { EventEmitter } from "node:events";
import EE from "node:events";

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
