import { Readable } from "node:stream";
// flatMap(fn) accepts an async function returning an iterable/generator;
// chunks are flattened in order.
const out = await Readable.from([1, 2])
  .flatMap(async function* (n: number) {
    yield n;
    yield n * 100;
  })
  .toArray();
console.log("joined:", out.join(","));
