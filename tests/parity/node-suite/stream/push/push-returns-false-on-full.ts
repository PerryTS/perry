import { Readable } from "node:stream";
// push() returns true while the buffer is below highWaterMark, false
// once it crosses (i.e., the producer should pause).
const r = new Readable({ highWaterMark: 4, read() {} });
const results: boolean[] = [];
results.push(r.push("aa"));
results.push(r.push("bb"));
results.push(r.push("cc"));
console.log("results:", results.join(","));
console.log("any false:", results.some((x) => x === false));
