import { Readable } from "node:stream";
// try { for await ... } finally — the finally runs even when iteration is
// abandoned and the stream is auto-destroyed.
const r = Readable.from(["a", "b", "c"]);
try {
  for await (const c of r) {
    if (c === "a") throw new Error("bail");
    console.log("got:", c);
  }
} catch {
  console.log("threw, destroyed:", r.destroyed);
}
