import { Readable, finished } from "node:stream";
// `{ error: false }` skips the extra error listener, but Node still calls the
// callback from `close` with the stream's stored destroy error.
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
