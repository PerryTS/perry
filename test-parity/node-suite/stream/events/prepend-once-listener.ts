import { Readable } from "node:stream";
// prependOnceListener inserts the listener at the front and removes it
// after one firing.
const r = new Readable({ read() {} });
const order: string[] = [];
r.on("custom", () => order.push("on"));
r.prependOnceListener("custom", () => order.push("prepend-once"));
r.emit("custom");
r.emit("custom"); // prepend-once should not fire again
console.log("order:", order.join(","));
console.log("count after 2 emits:", order.length);
