import { Readable, PassThrough } from "node:stream";
// One source can pipe to multiple destinations; each gets the data.
const r = Readable.from(["x"]);
const a = new PassThrough();
const b = new PassThrough();
let aGot = 0, bGot = 0;
a.on("data", () => aGot++);
b.on("data", () => bGot++);
let endCount = 0;
const checkDone = () => {
  endCount++;
  if (endCount === 2) console.log("a:", aGot, "b:", bGot);
};
a.on("end", checkDone);
b.on("end", checkDone);
r.pipe(a);
r.pipe(b);
