// Gap test: #10543 — a ReadStream open error must be emitted after the caller
// has had time to attach listeners to the returned stream.
import fs from "node:fs";

const missing = "/tmp/perry_gap_10543_missing/nope.txt";
fs.rmSync("/tmp/perry_gap_10543_missing", { recursive: true, force: true });

fs.createReadStream(missing)
  .on("data", () => console.log("unexpected data"))
  .on("error", (error: any) => console.log("error event", error.code));

console.log("sync after createReadStream");
