import { EventEmitter } from "node:events";

const em = new EventEmitter();
console.log("default:", em.getMaxListeners());
console.log("chain:", em.setMaxListeners(42) === em);
console.log("updated:", em.getMaxListeners());
try { em.setMaxListeners(-1); console.log("negative no throw"); } catch (err: any) { console.log("negative:", err?.name, err?.code || "no-code"); }
console.log("after negative:", em.getMaxListeners());
