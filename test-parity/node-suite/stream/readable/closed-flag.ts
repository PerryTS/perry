import { Readable } from "node:stream";
// readable.closed becomes true after the close event (post end+destroy).
const r = Readable.from(["x"]);
r.on("close", () => console.log("closed:", r.closed));
r.on("data", () => {});
