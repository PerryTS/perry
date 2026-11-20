import { EventEmitter } from "node:events";

const em = new EventEmitter();
const seen: string[] = [];
em.on("newListener", (name: string) => seen.push(name));
em.on("hello", () => {});
em.once("once", () => {});
console.log("seen:", seen);
