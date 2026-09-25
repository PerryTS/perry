// #11270 / #11273: an EventEmitter read back out of an array (or any receiver whose
// static type the compiler cannot prove) must dispatch to the same emitter.
// Default `node:events` routes to perry-ext-events; the prebuilt stdlib's
// dynamic dispatcher used to consult its own (empty) registry and silently
// no-op every `.on` / `.emit`.
import { EventEmitter } from "node:events";
import * as events from "events";

// 1. The reported shapes: push in a loop, index read, local alias.
const emitters: EventEmitter[] = [];
for (let i = 0; i < 2; i++) emitters.push(new EventEmitter());
let fired = 0;
emitters[0].on("x", (n: number) => (fired += n));
emitters[0].emit("x", 5);
console.log("array elem fired:", fired);
const e = emitters[1];
let f2 = 0;
e.on("x", (n: number) => (f2 += n));
e.emit("x", 5);
console.log("local fired:", f2);

// 2. Identity: a listener added through the array fires through the
// original binding, and vice versa.
const orig = new EventEmitter();
const held: EventEmitter[] = [orig];
let viaOrig = 0;
held[0].on("y", (a: number, b: number) => (viaOrig += a * b));
orig.emit("y", 3, 4);
orig.on("z", () => (viaOrig += 100));
held[0].emit("z");
console.log("identity:", viaOrig, held[0] === orig);

// 3. Array literal, untyped array, any[], for...of, Map values.
const lit: EventEmitter[] = [new EventEmitter()];
const untyped = [new EventEmitter()];
const anys: any[] = [new EventEmitter()];
let total = 0;
lit[0].on("t", () => (total += 1));
untyped[0].on("t", () => (total += 10));
anys[0].on("t", () => (total += 100));
for (const em of [lit[0], untyped[0], anys[0]]) em.emit("t");
const byName = new Map<string, EventEmitter>([["a", new EventEmitter()]]);
byName.get("a")!.on("t", () => (total += 1000));
byName.get("a")!.emit("t");
console.log("containers:", total);

// 4. Class field and destructuring.
class Holder {
  em: EventEmitter = new EventEmitter();
}
const h = new Holder();
const [first] = emitters;
let other = 0;
h.em.on("q", (v: string) => (other += v.length));
h.em.emit("q", "abcd");
first.emit("x", 1);
console.log("field/destructure:", other, fired);

// 5. The rest of the listener surface through a dynamic receiver.
const d = emitters[1];
const log: string[] = [];
const onA = () => log.push("a");
d.once("o", () => log.push("once"));
d.prependListener("o", () => log.push("pre"));
d.prependOnceListener("o", () => log.push("preOnce"));
d.addListener("o", onA);
console.log("emit returns:", d.emit("o"), d.emit("o"), d.emit("nobody"));
console.log("order:", log.join(","));
console.log("count:", d.listenerCount("o"), d.listeners("o").length, d.rawListeners("o").length);
console.log("names:", d.eventNames().join(","));
d.off("o", onA);
console.log("after off:", d.listenerCount("o"));
d.removeListener("o", onA);
d.setMaxListeners(3);
console.log("max:", d.getMaxListeners());
d.removeAllListeners("o");
console.log("after removeAll(o):", d.listenerCount("o"), d.eventNames().join(","));
d.removeAllListeners();
console.log("after removeAll():", d.eventNames().length);
console.log("chain:", d.on("c", () => {}) === d, d.setMaxListeners(4) === d);

// 6. Method values read off a dynamic receiver stay bound.
const em2 = emitters[0];
const on = em2.on;
const emit = em2.emit;
let bound = 0;
on.call(em2, "b", () => (bound += 1));
console.log("bound emit:", emit.call(em2, "b"), bound, typeof em2.listenerCount);

// 7. 'error' with no listener still throws through a dynamic receiver.
try {
  emitters[0].emit("error", new Error("boom"));
  console.log("no throw");
} catch (err) {
  console.log("threw:", (err as Error).message);
}

// 8. #11273: `new events.EventEmitter()` through a NAMESPACE import is a
// dynamic receiver too (constructed via the runtime's native-construct hook).
const ns = new events.EventEmitter();
const local = new EventEmitter();
console.log("ns typeof:", typeof ns.on, typeof ns.listenerCount, typeof local.on);
ns.on("x", () => console.log("ns fired"));
console.log("ns count:", events.getEventListeners(ns, "x").length, ns.listenerCount("x"));
ns.emit("x");
events.once(ns, "y").then((args) => console.log("ns once resolved:", args.length));
ns.emit("y", 1, 2);
