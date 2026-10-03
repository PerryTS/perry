// #11827: Readable.from(array) reads the array; it must not empty it.
import { Readable } from "node:stream";

const arr = ["a", "b", "c"];
const first: string[] = [];
for await (const chunk of Readable.from(arr)) first.push(String(chunk));
console.log(first.join(""), arr.length, JSON.stringify(arr));

const second: string[] = [];
for await (const chunk of Readable.from(arr)) second.push(String(chunk));
console.log(second.join(""), arr.length);

const bufs = [new Uint8Array([1, 2]), new Uint8Array([3])];
const bytes: number[] = [];
for await (const chunk of Readable.from(bufs)) bytes.push(...(chunk as Uint8Array));
console.log(bytes.join(","), bufs.length, bufs[1][0]);

// A stream that is only partly read leaves the array alone too.
const nums = [1, 2, 3, 4];
const partial = Readable.from(nums);
for await (const n of partial) {
  if (n === 2) break;
}
console.log(nums.join(","));
