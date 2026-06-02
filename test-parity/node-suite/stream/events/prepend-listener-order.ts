import { Readable } from "node:stream";
// prependListener(event, fn) inserts fn at the front of the listener
// array, so it fires BEFORE any listener added via on().
const r = Readable.from(["x"]);
const order: string[] = [];
r.on("data", () => order.push("on"));
r.prependListener("data", () => order.push("prepend"));
r.on("end", () => console.log("order:", order.join(",")));
