import { Readable } from "node:stream";
// readable.some(fn) resolves true if any chunk matches, false otherwise.
const any = await Readable.from([1, 2, 3]).some((n: number) => n > 2);
const none = await Readable.from([1, 2, 3]).some((n: number) => n > 100);
console.log("any > 2:", any);
console.log("any > 100:", none);
