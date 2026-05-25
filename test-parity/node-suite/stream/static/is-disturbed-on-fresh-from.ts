import { Readable, isDisturbed } from "node:stream";
// Readable.from() stream is not disturbed until consumed.
const r = Readable.from([1, 2, 3]);
console.log("fresh from:", isDisturbed(r));
console.log("is false:", isDisturbed(r) === false);
