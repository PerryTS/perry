import { Readable } from "node:stream";
// The default for the iterator is destroyOnReturn: true — early-return
// destroys the stream.
const r = Readable.from(["a", "b", "c"]);
let count = 0;
for await (const _v of r) {
  count++;
  if (count === 1) break; // early return
}
setImmediate(() => {
  console.log("destroyed after early break:", r.destroyed);
});
