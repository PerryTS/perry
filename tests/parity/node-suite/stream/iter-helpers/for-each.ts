import { Readable } from "node:stream";
// readable.forEach(fn) consumes the stream calling fn for each chunk; resolves
// with undefined.
const seen: number[] = [];
const ret = await Readable.from([1, 2, 3]).forEach((n: number) => { seen.push(n); });
console.log("ret:", ret);
console.log("seen:", seen.join(","));
