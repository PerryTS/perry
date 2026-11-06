import { EventEmitter } from "node:events";

const em = new EventEmitter();
const seen: string[] = [];
em.on("removeListener", (name: string) => seen.push(name));
em.on("x", () => {});
em.on("x", () => {});
em.removeAllListeners("x");
console.log("seen:", seen);
