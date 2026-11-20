import { PassThrough } from "node:stream";
// readable.pipe(writable) returns the writable for chaining.
const src = new PassThrough();
const sink = new PassThrough();
const ret = src.pipe(sink);
console.log("returns target:", ret === sink);
