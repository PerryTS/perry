import { EventEmitter } from "node:events";

const em = new EventEmitter();
em.on("a", () => {});
em.on("b", () => {});
em.removeAllListeners();
console.log("names:", em.eventNames());
console.log("count a:", em.listenerCount("a"));
