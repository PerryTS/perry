import { Readable } from "node:stream";
// The 'readable' event fires when data is available; an extra 'readable'
// also fires right before 'end' (so user can detect end via read() === null).
const order: string[] = [];
const r = new Readable({ read() {} });
r.on("readable", () => order.push("readable"));
r.on("end", () => {
  order.push("end");
  console.log("order:", order.join(","));
  console.log("readable fired before end:", order.indexOf("readable") < order.indexOf("end"));
});
r.push("x");
r.push(null);
// Trigger the readable->end transition
setImmediate(() => { r.read(); r.read(); });
