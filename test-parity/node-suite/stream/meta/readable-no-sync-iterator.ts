import { Readable } from "node:stream";
// Readable streams expose Symbol.asyncIterator but NOT Symbol.iterator
// (they're async by nature).
const r = new Readable({ read() {} });
console.log("asyncIterator typeof:", typeof (r as any)[Symbol.asyncIterator]);
console.log("sync iterator typeof:", typeof (r as any)[Symbol.iterator]);
console.log("sync is undefined:", (r as any)[Symbol.iterator] === undefined);
