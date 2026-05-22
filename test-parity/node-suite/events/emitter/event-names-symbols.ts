import { EventEmitter } from "node:events";

const ee = new EventEmitter();
const sym = Symbol("evt");
ee.on("a", () => {});
ee.on(sym, () => {});
console.log("names:", ee.eventNames().map(String).sort().join(","));
console.log("count sym:", ee.listenerCount(sym));
ee.removeAllListeners("a");
console.log("names after:", ee.eventNames().map(String).join(","));
