import { Readable } from "node:stream";
// Readable instances expose Symbol.asyncIterator but NOT Symbol.iterator —
// they're async-only.
const r = new Readable({ read() {} });
console.log("has Symbol.asyncIterator:", typeof (r as any)[Symbol.asyncIterator] === "function");
console.log("Symbol.iterator missing:", (r as any)[Symbol.iterator] === undefined);
