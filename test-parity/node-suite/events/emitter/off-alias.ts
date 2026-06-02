import { EventEmitter } from "node:events";

const em = new EventEmitter();
const seen: number[] = [];
const h = (n: number) => seen.push(n);
em.on("x", h);
em.emit("x", 1);
console.log("off chain:", em.off("x", h) === em);
em.emit("x", 2);
console.log("seen:", seen);
