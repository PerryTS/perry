import { Readable } from "node:stream";
// pause() immediately flips readableFlowing to false (after the stream
// was flowing).
const r = Readable.from(["x"]);
r.on("data", () => {});
console.log("flowing after data listener:", r.readableFlowing);
r.pause();
console.log("flowing after pause:", r.readableFlowing);
console.log("is false:", r.readableFlowing === false);
