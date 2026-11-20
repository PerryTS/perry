import { EventEmitter } from "node:events";

const ee = new EventEmitter();
const events: string[] = [];
function a() { events.push("a"); ee.off("x", b); ee.on("x", c); }
function b() { events.push("b"); }
function c() { events.push("c"); }
ee.on("x", a);
ee.on("x", b);
ee.emit("x");
ee.emit("x");
console.log("events:", events.join(","));
