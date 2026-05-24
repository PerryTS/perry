import { Readable, PassThrough } from "node:stream";
// unpipe() with no arg removes ALL pipe destinations from the source.
const r = Readable.from(["x"]);
const a = new PassThrough();
const b = new PassThrough();
let aGot = 0, bGot = 0;
a.on("data", () => aGot++);
b.on("data", () => bGot++);
r.pipe(a);
r.pipe(b);
r.unpipe();
setImmediate(() => {
  setImmediate(() => {
    console.log("a got after unpipe:", aGot);
    console.log("b got after unpipe:", bGot);
  });
});
