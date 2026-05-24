import { Readable } from "node:stream";
// `await using` triggers Symbol.asyncDispose at the end of the scope —
// the stream is destroyed after the block.
let r: Readable;
{
  await using inner = Readable.from(["a", "b"]) as any;
  r = inner;
  for await (const _ of inner) {
    /* consume to keep the stream live */
  }
}
console.log("destroyed after scope:", r!.destroyed);
