import { Transform } from "node:stream";
// Transform in objectMode transforms one JS value at a time.
const doubler = new Transform({
  objectMode: true,
  transform(n, _e, cb) { cb(null, n * 2); },
});
const out: number[] = [];
doubler.on("data", (n) => out.push(n));
doubler.on("end", () => console.log("doubled:", out.join(",")));
doubler.write(1);
doubler.write(2);
doubler.write(3);
doubler.end();
