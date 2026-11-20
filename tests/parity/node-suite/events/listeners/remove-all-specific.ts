import { EventEmitter } from "node:events";

const em = new EventEmitter();
em.on("a", () => {});
em.on("b", () => {});
console.log("chain:", em.removeAllListeners("a") === em);
console.log("names:", em.eventNames());
console.log("a count:", em.listenerCount("a"));
console.log("b count:", em.listenerCount("b"));
