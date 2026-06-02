import { Readable } from "node:stream";
// Two concurrent iterators on the same Readable should error — the stream
// can only be consumed by one iterator at a time.
const r = Readable.from(["a", "b"]);
const a = (r as any)[Symbol.asyncIterator]();
let secondThrew = false;
try {
  const b = (r as any)[Symbol.asyncIterator]();
  await b.next();
} catch {
  secondThrew = true;
}
await a.next();
console.log("concurrent threw:", secondThrew);
