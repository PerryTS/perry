import { Readable } from "node:stream";
// readable.find(fn) resolves to the first matching chunk, or undefined.
const v = await Readable.from([1, 2, 3, 4]).find((n: number) => n > 2);
console.log("first > 2:", v);
