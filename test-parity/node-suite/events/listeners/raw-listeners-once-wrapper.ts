import { EventEmitter } from "node:events";

const em = new EventEmitter();
const calls: string[] = [];
function h(value: string) {
  calls.push(value);
}

em.once("x", h);
const listeners = em.listeners("x");
const raw = em.rawListeners("x");
const rawAgain = em.rawListeners("x");

console.log("listeners original:", listeners[0] === h);
console.log("raw original:", raw[0] === h);
console.log("raw listener prop:", (raw[0] as any).listener === h);
console.log("raw stable:", raw[0] === rawAgain[0]);
console.log("count original:", em.listenerCount("x", h));
console.log("count raw:", em.listenerCount("x", raw[0]));

(raw[0] as any)("manual");
console.log("calls:", calls.join("|"));
console.log("count after raw call:", em.listenerCount("x"));

em.once("x", h);
const rawRemove = em.rawListeners("x")[0];
em.removeListener("x", rawRemove as any);
console.log("remove raw count:", em.listenerCount("x"));

em.once("x", h);
em.removeListener("x", h);
console.log("remove original count:", em.listenerCount("x"));
