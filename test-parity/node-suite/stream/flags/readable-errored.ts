import { Readable } from "node:stream";
// readable.errored stores the Error passed to destroy(err) (or null if
// no error). Distinct from the static Readable.isErrored() helper.
const r = new Readable({ read() {} });
r.on("error", () => {});
console.log("errored before:", r.errored);
r.destroy(new Error("kaboom"));
setImmediate(() => {
  console.log("errored typeof:", typeof r.errored);
  console.log("errored message:", r.errored ? (r.errored as Error).message : null);
});
