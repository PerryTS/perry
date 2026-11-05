import { Readable } from "node:stream";
// read(0) is a no-op probe — it does NOT consume bytes but may trigger
// a 'readable' event when data is available.
let readableFired = 0;
const r = new Readable({ read() {} });
r.on("readable", () => readableFired++);
r.push("data");
const r0 = r.read(0);
console.log("read(0):", r0); // null — no consumption
console.log("readable fired:", readableFired > 0);
// data still available
const remaining = r.read();
console.log("remaining length:", remaining && remaining.length);
