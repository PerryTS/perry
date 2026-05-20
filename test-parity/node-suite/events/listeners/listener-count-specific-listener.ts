import { EventEmitter } from "node:events";

const em = new EventEmitter();
const h = () => {};
const g = () => {};
em.on("x", h);
em.on("x", h);
em.on("x", g);
console.log("total:", em.listenerCount("x"));
console.log("h count:", em.listenerCount("x", h));
console.log("g count:", em.listenerCount("x", g));
