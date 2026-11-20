import { EventEmitter, on } from "node:events";

const em = new EventEmitter();
const iter = on(em, "data");
em.emit("data", 1);
em.emit("data", 2);
em.emit("data", 3);
const seen: number[] = [];
for await (const args of iter) {
  seen.push(args[0]);
  if (seen.length === 3) break;
}
console.log("seen:", seen);
