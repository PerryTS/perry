import { EventEmitter } from "node:events";

const em = new EventEmitter();
console.log("default:", em.getMaxListeners());
console.log("chain:", em.setMaxListeners(42) === em);
console.log("updated:", em.getMaxListeners());
