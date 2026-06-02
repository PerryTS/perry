import { Duplex } from "node:stream";
import { finished } from "node:stream/promises";
// finished() on a Duplex resolves once both sides settle.
const d = new Duplex({
  read() { this.push(null); },
  write(_c, _e, cb) { cb(); },
});
d.on("data", () => {});
const p = finished(d);
d.end("x");
const result = await p;
console.log("resolved:", result === undefined);
