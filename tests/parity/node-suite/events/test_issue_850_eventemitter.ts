// Issue #850 - EventEmitter probes (subset).
import * as events from "node:events";
import { EventEmitter } from "node:events";

// 1. listenerCount + eventNames
const em = new EventEmitter();
em.on("inc", (_n: number) => {});
em.on("inc", (_n: number) => {});
console.log("listenerCount('inc'):", em.listenerCount("inc"));
console.log("eventNames():", em.eventNames());

// 2. once
const onceVals: number[] = [];
em.once("solo", (n: number) => { onceVals.push(n); });
em.emit("solo", 7);
em.emit("solo", 8);
console.log("once values:", onceVals);

// 3. addListener / removeListener (aliases)
const addSeen: number[] = [];
const handler = (n: number) => { addSeen.push(n); };
em.addListener("alias", handler);
em.emit("alias", 1);
em.removeListener("alias", handler);
em.emit("alias", 2);
console.log("addListener/removeListener:", addSeen);

// 4. prependListener order
const order: string[] = [];
em.on("ord", () => { order.push("b"); });
em.prependListener("ord", () => { order.push("a"); });
em.emit("ord");
console.log("prependListener order:", order);

// 5. prependOnceListener order
const onceOrder: string[] = [];
em.on("ord2", () => { onceOrder.push("b"); });
em.prependOnceListener("ord2", () => { onceOrder.push("a"); });
em.emit("ord2");
em.emit("ord2");
console.log("prependOnceListener order:", onceOrder);

// 6. listeners / rawListeners
console.log("listeners count:", em.listeners("inc").length);
console.log("rawListeners count:", em.rawListeners("inc").length);

// 7. setMaxListeners / getMaxListeners
em.setMaxListeners(42);
console.log("getMaxListeners:", em.getMaxListeners());

// 8. removeAllListeners
em.removeAllListeners();
console.log("after removeAllListeners eventNames:", em.eventNames());

// 9. events.once static helper
try {
  const em2 = new EventEmitter();
  const p = events.once(em2, "ready");
  em2.emit("ready", "value-1");
  const result = await p;
  console.log("events.once length:", (result as any[]).length);
} catch (e: any) {
  console.log("events.once ERR:", e && e.message);
}

// 10. events.getEventListeners
try {
  const em3 = new EventEmitter();
  em3.on("x", () => {});
  em3.on("x", () => {});
  const ls = events.getEventListeners(em3, "x");
  console.log("events.getEventListeners count:", ls.length);
} catch (e: any) {
  console.log("events.getEventListeners ERR:", e && e.message);
}
