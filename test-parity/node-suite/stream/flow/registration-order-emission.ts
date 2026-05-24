import { Readable } from "node:stream";
// Multiple 'data' listeners fire in registration order for each chunk.
const r = Readable.from(["x"]);
const order: string[] = [];
r.on("data", () => order.push("first"));
r.on("data", () => order.push("second"));
r.on("data", () => order.push("third"));
r.on("end", () => console.log("order:", order.join(",")));
