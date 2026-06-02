import { PassThrough } from "node:stream";
// prependOnceListener inserts a once-listener at the front (runs first
// then auto-removes).
const order: number[] = [];
const p = new PassThrough();
p.on("data", () => order.push(2));
p.prependOnceListener("data", () => order.push(1));
p.write("a");
p.write("b");
p.end(() => console.log("order:", order.join(",")));
