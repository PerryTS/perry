import { Readable } from "node:stream";
// map(fn, { concurrency }) runs the async transform with bounded parallelism.
const out = await Readable.from([1, 2, 3, 4])
  .map(async (n: number) => n * 10, { concurrency: 2 })
  .toArray();
console.log("joined:", out.sort().join(","));
