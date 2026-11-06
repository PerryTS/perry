import { EventEmitter } from "node:events";

const em = new EventEmitter();
const seen: string[] = [];
em.on("error", (err: Error) => seen.push(err.message));
console.log("emit returned:", em.emit("error", new Error("handled")));
console.log("seen:", seen);
