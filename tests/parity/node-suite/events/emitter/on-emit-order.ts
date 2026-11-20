import { EventEmitter } from "node:events";

const em = new EventEmitter();
const seen: number[] = [];
em.on("inc", (n: number) => seen.push(n));
em.on("inc", (n: number) => seen.push(n * 10));
console.log("emit one:", em.emit("inc", 2));
console.log("seen:", seen);
console.log("emit missing:", em.emit("missing"));
