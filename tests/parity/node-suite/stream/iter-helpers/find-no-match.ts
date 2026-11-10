import { Readable } from "node:stream";
// find(fn) returns undefined when no item matches the predicate.
const r = Readable.from([1, 2, 3, 4]);
const result = await (r as any).find((x: number) => x > 10);
console.log("found:", result);
console.log("is undefined:", result === undefined);
