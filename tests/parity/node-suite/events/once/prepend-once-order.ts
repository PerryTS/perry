import { EventEmitter } from "node:events";

const em = new EventEmitter();
const order: string[] = [];
em.on("x", () => order.push("tail"));
em.prependOnceListener("x", () => order.push("head-once"));
em.emit("x");
em.emit("x");
console.log("order:", order);
console.log("count:", em.listenerCount("x"));
