import { Readable, finished } from "node:stream";
// finished(stream, { error: false }, cb) — should NOT fire the callback
// when the stream emits an error (only on normal completion).
const r = new Readable({ read() {} });
r.on("error", () => {});
let fired = false;
let firedWith: any = null;
finished(r, { error: false } as any, (err: any) => {
  fired = true;
  firedWith = err;
});
r.destroy(new Error("kaboom"));
setImmediate(() => console.log("fired:", fired, "with:", firedWith));
