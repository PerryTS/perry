import { EventEmitter, getMaxListeners, setMaxListeners } from "node:events";

const a = new EventEmitter();
const b = new EventEmitter();
console.log("before:", getMaxListeners(a), getMaxListeners(b));
setMaxListeners(3, a, b);
console.log("after:", getMaxListeners(a), getMaxListeners(b));
try { setMaxListeners(-1, a); console.log("negative no throw"); } catch (err: any) { console.log("negative:", err?.name, err?.code || "no-code"); }
