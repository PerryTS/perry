import { Readable } from "node:stream";
// reduce(fn) without an initial value uses the first stream item as the
// accumulator, then folds the rest.
const r = Readable.from([1, 2, 3, 4]);
const sum = await (r as any).reduce((acc: number, x: number) => acc + x);
console.log("sum:", sum);
