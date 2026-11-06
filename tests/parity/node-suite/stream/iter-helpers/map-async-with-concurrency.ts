import { Readable } from "node:stream";
// map(asyncFn, {concurrency:2}) — supports concurrency option.
const r = Readable.from([1, 2, 3, 4]);
const result = await (r as any).map(
  async (x: number) => {
    await new Promise((resolve) => setTimeout(resolve, 5));
    return x * 10;
  },
  { concurrency: 2 },
).toArray();
console.log("result:", result.join(","));
