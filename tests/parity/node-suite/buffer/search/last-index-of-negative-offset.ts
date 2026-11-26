import { Buffer } from "node:buffer";

// A byteOffset below -length searches nothing (#15440); one in range counts
// from the end and never lets a multi-byte needle run past the data.
const b = Buffer.from([7, 8, 7]);
for (const off of [-10, -4, -Infinity, -3, -2, -1, 0, 1, 2, 5]) {
  console.log(String(off), b.lastIndexOf(7, off), b.lastIndexOf(Buffer.from([7, 8]), off),
    b.lastIndexOf(Buffer.from([8, 7]), off), b.lastIndexOf("", off));
}
const s = Buffer.from("abcabc");
console.log("str", s.lastIndexOf("bc", -1), s.lastIndexOf("bc", -6), s.lastIndexOf("bc", -7), s.lastIndexOf("abc", -2));
