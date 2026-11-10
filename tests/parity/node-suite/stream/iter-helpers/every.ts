import { Readable } from "node:stream";
// readable.every(fn) resolves true iff every chunk matches.
const all = await Readable.from([2, 4, 6]).every((n: number) => n % 2 === 0);
const not = await Readable.from([2, 3, 4]).every((n: number) => n % 2 === 0);
console.log("all even:", all);
console.log("mixed:", not);
