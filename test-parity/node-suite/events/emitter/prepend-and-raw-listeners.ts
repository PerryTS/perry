import { EventEmitter } from "node:events";

const ee = new EventEmitter();
const events: string[] = [];
function a() { events.push("a"); }
function b() { events.push("b"); }
ee.on("x", a);
ee.prependListener("x", b);
ee.once("x", () => events.push("once"));
console.log("listeners:", ee.listeners("x").length, ee.rawListeners("x").length);
ee.emit("x");
ee.emit("x");
console.log("events:", events.join(","));
