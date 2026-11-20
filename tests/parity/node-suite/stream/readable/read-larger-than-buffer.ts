import { Readable } from "node:stream";
// read(n) with n larger than the buffered bytes returns null (waits for
// more) when the stream isn't ended; once ended, returns the remaining.
const r = new Readable({ read() {} });
r.push(Buffer.from("ab"));
r.push(null);
const big = r.read(100);
console.log("got length:", big && big.length);
