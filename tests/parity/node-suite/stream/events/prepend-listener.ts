import { PassThrough } from "node:stream";
// prependListener inserts a listener at the front; it runs before others.
const order: number[] = [];
const p = new PassThrough();
p.on("data", () => order.push(2));
p.prependListener("data", () => order.push(1));
p.write("x");
p.end();
setImmediate(() => console.log("order:", order.join(",")));
