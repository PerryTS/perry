import { Readable } from "node:stream";
// eventNames() returns the array of event names currently with at least
// one listener (string + symbol entries).
const r = new Readable({ read() {} });
r.on("data", () => {});
r.on("end", () => {});
const names = r.eventNames();
console.log("is array:", Array.isArray(names));
console.log("count:", names.length);
console.log("has data:", names.includes("data"));
console.log("has end:", names.includes("end"));
