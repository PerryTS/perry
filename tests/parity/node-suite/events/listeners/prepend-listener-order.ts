import { EventEmitter } from "node:events";

const em = new EventEmitter();
const order: string[] = [];
em.on("x", () => order.push("b"));
em.prependListener("x", () => order.push("a"));
em.on("x", () => order.push("c"));
em.emit("x");
console.log("order:", order);
