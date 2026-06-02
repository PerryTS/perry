import { EventEmitter, listenerCount } from "node:events";

const em = new EventEmitter();
em.on("x", () => {});
em.on("x", () => {});
em.on("y", () => {});
console.log("instance x:", em.listenerCount("x"));
console.log("instance missing:", em.listenerCount("missing"));
console.log("events x:", listenerCount(em, "x"));
console.log("static x:", EventEmitter.listenerCount(em, "x"));
