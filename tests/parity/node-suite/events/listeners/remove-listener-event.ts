import { EventEmitter } from "node:events";

const ee = new EventEmitter();
const events: string[] = [];
function fn() {}
ee.on("removeListener", (name, listener) => events.push(String(name) + ":" + (listener === fn)));
ee.on("x", fn);
ee.off("x", fn);
ee.removeAllListeners("removeListener");
console.log("events:", events.join(","));
