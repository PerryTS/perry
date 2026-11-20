import { Writable } from "node:stream";
// end(chunkA); end(chunkB) — second end's chunk is ignored.
const received: string[] = [];
const w = new Writable({
  write(c, _e, cb) { received.push(String(c)); cb(); },
});
w.end("first");
w.end("second"); // ignored
w.on("finish", () => {
  console.log("received:", received.join(","));
});
