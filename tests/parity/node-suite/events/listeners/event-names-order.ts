import { EventEmitter } from "node:events";

const em = new EventEmitter();
em.on("b", () => {});
em.on("a", () => {});
em.on("b", () => {});
console.log("names:", em.eventNames());
em.removeAllListeners("b");
console.log("after prune:", em.eventNames());
