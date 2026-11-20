import { Readable } from "node:stream";
// Fresh stream — eventNames returns empty array.
const r = new Readable({ read() {} });
const names = r.eventNames();
console.log("is array:", Array.isArray(names));
console.log("length:", names.length);
console.log("empty:", names.length === 0);
