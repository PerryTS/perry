import { EventEmitter } from "node:events";

const em = new EventEmitter();
let count = 0;
const h = () => { count++; };
em.once("x", h);
em.removeListener("x", h);
em.emit("x");
console.log("count:", count);
console.log("listeners:", em.listenerCount("x"));
