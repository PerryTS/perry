import { EventEmitter } from "node:events";

const ee = new EventEmitter();
const events: string[] = [];
ee.once("x", () => events.push("once"));
ee.prependOnceListener("x", () => events.push("prependOnce"));
ee.prependListener("x", () => events.push("prepend"));
ee.emit("x");
ee.emit("x");
console.log("events:", events.join(","));
