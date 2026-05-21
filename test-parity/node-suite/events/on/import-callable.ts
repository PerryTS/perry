import { EventEmitter } from "node:events";
import * as events from "node:events";

const em = new EventEmitter();
const iter = events.on(em, "tick");
em.emit("tick", "a");
em.emit("tick", "b");
const seen: string[] = [];
for await (const args of iter) {
  seen.push(args[0]);
  if (seen.length === 2) break;
}
console.log("namespace on type:", typeof events.on);
console.log("seen:", seen.join(","));
