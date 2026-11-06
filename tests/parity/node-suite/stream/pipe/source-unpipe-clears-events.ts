import { Readable, PassThrough } from "node:stream";
// unpipe(dst) — clears the internal pipe state.
const r = Readable.from(["x"]);
const dst = new PassThrough();
dst.on("data", () => {});
r.pipe(dst);
r.unpipe(dst);
// Push directly to source — dst shouldn't receive
const collected: string[] = [];
dst.on("data", (c) => collected.push(String(c)));
setImmediate(() => {
  setImmediate(() => {
    console.log("dst received after unpipe:", collected.length);
  });
});
