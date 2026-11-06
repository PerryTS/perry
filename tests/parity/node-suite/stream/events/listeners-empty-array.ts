import { Readable } from "node:stream";
// listeners(event) with no listeners — returns empty array.
const r = new Readable({ read() {} });
const arr = r.listeners("no-listener-here");
console.log("is array:", Array.isArray(arr));
console.log("length:", arr.length);
console.log("is empty:", arr.length === 0);
