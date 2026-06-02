import { Readable } from "node:stream";
// autoDestroy: true (default in modern Node) destroys the stream after end.
const r = Readable.from(["x"]);
r.on("data", () => {});
r.on("close", () => console.log("destroyed after end:", r.destroyed));
