import { EventEmitter, getEventListeners } from "node:events";

const em = new EventEmitter();
const h = () => {};
em.on("x", h);
const list = em.listeners("x");
const copy = getEventListeners(em, "x");
list.pop();
console.log("same handler:", list[0] === h || copy[0] === h);
console.log("live count:", em.listenerCount("x"));
console.log("copy count:", copy.length);
