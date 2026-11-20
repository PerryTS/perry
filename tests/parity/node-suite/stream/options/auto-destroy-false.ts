import { Readable } from "node:stream";
// autoDestroy: false keeps the stream alive after end (no automatic destroy).
const r = new Readable({ autoDestroy: false, read() {} });
r.push("x");
r.push(null);
r.on("data", () => {});
r.on("end", () => console.log("destroyed:", r.destroyed));
