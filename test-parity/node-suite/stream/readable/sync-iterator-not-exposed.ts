import { Readable } from "node:stream";
// Symbol.iterator should not be exposed on Readable — only Symbol.asyncIterator.
const r = Readable.from(["a"]);
const sync = (r as any)[Symbol.iterator];
const asyncIter = (r as any)[Symbol.asyncIterator];
console.log("sync iterator typeof:", typeof sync);
console.log("asyncIterator typeof:", typeof asyncIter);
console.log("sync is undefined:", sync === undefined);
