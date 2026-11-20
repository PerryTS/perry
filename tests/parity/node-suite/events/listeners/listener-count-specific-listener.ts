import { EventEmitter } from "node:events";

const em = new EventEmitter();
const h = () => {};
const g = () => {};
em.on("x", h);
em.on("x", h);
em.once("x", h);
em.prependOnceListener("x", g);
const rawH = em.rawListeners("x").filter((listener) => (listener as any).listener === h)[0];
console.log("total:", em.listenerCount("x"));
console.log("h count:", em.listenerCount("x", h));
console.log("g count:", em.listenerCount("x", g));
console.log("raw h count:", em.listenerCount("x", rawH));
console.log("undefined filter:", em.listenerCount("x", undefined as any));
console.log("null filter:", em.listenerCount("x", null as any));
console.log("object filter:", em.listenerCount("x", {} as any));
