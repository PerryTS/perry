import { EventEmitter } from "node:events";

const em = new EventEmitter();
const h = () => {};
em.on("x", h);
em.removeListener("missing", h);
console.log("x count:", em.listenerCount("x"));
console.log("missing count:", em.listenerCount("missing"));
