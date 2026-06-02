import { EventEmitter, once } from "node:events";

const em = new EventEmitter();
const p = once(em, "ready");
em.emit("ready");
const result = await p;
console.log("length:", result.length);
