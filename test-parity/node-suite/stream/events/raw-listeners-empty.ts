import { Readable } from "node:stream";
// rawListeners(no-listeners-event) — returns empty array.
const r = new Readable({ read() {} });
const arr = r.rawListeners("never");
console.log("is array:", Array.isArray(arr));
console.log("length:", arr.length);
console.log("is empty:", arr.length === 0);
