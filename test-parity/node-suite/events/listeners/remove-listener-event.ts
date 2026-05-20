import { EventEmitter } from "node:events";

const em = new EventEmitter();
const seen: string[] = [];
em.on("removeListener", (name: string) => seen.push(name));
const h = () => {};
em.on("hello", h);
em.removeListener("hello", h);
console.log("seen:", seen);
