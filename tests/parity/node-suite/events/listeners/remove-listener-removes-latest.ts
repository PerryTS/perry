import { EventEmitter } from "node:events";

const ee = new EventEmitter();
const order: string[] = [];

function h() {
  order.push("h");
}

function a() {
  order.push("a");
}

ee.on("x", h);
ee.on("x", a);
ee.on("x", h);

const before = ee.listeners("x");
console.log("before:", before[0] === h, before[1] === a, before[2] === h, before.length);
console.log("chain:", ee.removeListener("x", h) === ee);

const after = ee.listeners("x");
console.log("after:", after[0] === h, after[1] === a, after.length);

ee.emit("x");
console.log("order:", order.join(","));
