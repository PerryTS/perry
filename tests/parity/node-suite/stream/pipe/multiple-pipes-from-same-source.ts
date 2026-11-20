import { Readable, PassThrough } from "node:stream";
// Two pipes from same source — each dst sees all data.
const r = Readable.from(["x", "y"]);
const a = new PassThrough();
const b = new PassThrough();
let aGot: string[] = [];
let bGot: string[] = [];
a.on("data", (c) => aGot.push(String(c)));
b.on("data", (c) => bGot.push(String(c)));
r.pipe(a);
r.pipe(b);
let endCount = 0;
const checkDone = () => {
  if (++endCount === 2) {
    console.log("a:", aGot.join(","));
    console.log("b:", bGot.join(","));
  }
};
a.on("end", checkDone);
b.on("end", checkDone);
