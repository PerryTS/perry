import { Readable, finished } from "node:stream";
// finished(stream, { cleanup: true }, cb) still invokes the callback when the
// readable side completes.
const r = Readable.from(["x"]);
r.on("data", () => {});
finished(r, { cleanup: true } as any, () => {
  console.log("cleanup callback fired:", true);
});
