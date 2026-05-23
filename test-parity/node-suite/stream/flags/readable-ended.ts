import { Readable } from "node:stream";
// readableEnded is true after the final chunk is consumed and 'end' fires.
const r = Readable.from(["x"]);
r.on("data", () => {});
r.on("end", () => console.log("readableEnded:", r.readableEnded));
