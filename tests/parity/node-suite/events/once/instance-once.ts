import { EventEmitter } from "node:events";

const em = new EventEmitter();
const seen: number[] = [];
em.once("x", (n: number) => seen.push(n));
console.log("before:", em.listenerCount("x"));
em.emit("x", 1);
em.emit("x", 2);
console.log("seen:", seen);
console.log("after:", em.listenerCount("x"));
