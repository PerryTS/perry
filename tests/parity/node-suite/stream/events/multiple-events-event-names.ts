import { Readable } from "node:stream";
// Multiple events registered — eventNames returns all of them.
const r = new Readable({ read() {} });
r.on("data", () => {});
r.on("end", () => {});
r.on("error", () => {});
r.on("close", () => {});
const names = r.eventNames();
console.log("count:", names.length);
console.log("has data:", names.includes("data"));
console.log("has end:", names.includes("end"));
console.log("has error:", names.includes("error"));
console.log("has close:", names.includes("close"));
