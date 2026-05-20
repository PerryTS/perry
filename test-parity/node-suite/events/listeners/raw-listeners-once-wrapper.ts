import { EventEmitter } from "node:events";

const em = new EventEmitter();
const h = () => {};
em.on("x", h);
console.log("listeners count:", em.listeners("x").length);
console.log("raw count:", em.rawListeners("x").length);
console.log("same handler:", em.rawListeners("x")[0] === h);
