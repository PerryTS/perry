import { Readable } from "node:stream";
// A stream can cycle through paused → flowing → paused. The
// readableFlowing getter reflects each state.
const r = new Readable({ read() {} });
console.log("initial:", r.readableFlowing);
r.on("data", () => {});
console.log("after data:", r.readableFlowing);
r.pause();
console.log("after pause:", r.readableFlowing);
r.resume();
console.log("after resume:", r.readableFlowing);
