import { EventEmitter } from "node:events";

const em = new EventEmitter();
let count = 0;
em.once("x", () => { count++; });
em.removeAllListeners("x");
em.emit("x");
console.log("count:", count);
console.log("listeners:", em.listenerCount("x"));
