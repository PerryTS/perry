import { Readable } from "node:stream";
// readable.filter(fn) yields only chunks where fn(chunk) is truthy.
const out = await Readable.from([1, 2, 3, 4]).filter((n: number) => n % 2 === 0).toArray();
console.log("evens:", out.join(","));
