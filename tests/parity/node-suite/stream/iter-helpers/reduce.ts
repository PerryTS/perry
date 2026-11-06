import { Readable } from "node:stream";
// readable.reduce(fn, init) folds chunks into a single value.
const sum = await Readable.from([1, 2, 3, 4]).reduce((acc: number, n: number) => acc + n, 0);
console.log("sum:", sum);
