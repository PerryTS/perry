import { ReadableStream } from "node:stream/web";
// Web ReadableStream exposes Symbol.asyncIterator but NOT Symbol.iterator.
const rs = new ReadableStream({ start(c) { c.close(); } });
console.log("asyncIterator typeof:", typeof (rs as any)[Symbol.asyncIterator]);
console.log("sync iterator typeof:", typeof (rs as any)[Symbol.iterator]);
console.log("sync iter undefined:", (rs as any)[Symbol.iterator] === undefined);
