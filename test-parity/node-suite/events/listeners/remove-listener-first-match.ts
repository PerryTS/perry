import { EventEmitter } from "node:events";

const em = new EventEmitter();
let count = 0;
const h = () => { count++; };
em.on("x", h);
em.on("x", h);
em.removeListener("x", h);
em.emit("x");
console.log("count:", count);
console.log("remaining:", em.listenerCount("x"));
