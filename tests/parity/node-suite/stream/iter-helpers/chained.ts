import { Readable } from "node:stream";
// Iterator helpers are chainable: map → filter → take → toArray.
const out = await Readable.from([1, 2, 3, 4, 5, 6])
  .map((n: number) => n * 10)
  .filter((n: number) => n > 20)
  .take(2)
  .toArray();
console.log("joined:", out.join(","));
