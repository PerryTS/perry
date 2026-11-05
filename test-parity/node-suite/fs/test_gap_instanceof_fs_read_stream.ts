// `instanceof` against a non-class right operand walks the receiver's
// prototype chain. A node:fs read stream answers a self link as its
// prototype; the walk must end there instead of spinning to its hop cap.
// Without the cycle guard each check walks 100,000 hops (about 130 ms), so the
// loop below needs minutes and the harness's 10 s run timeout fails it; with the
// guard each check ends on the first hop and the loop takes milliseconds.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const file = path.join(os.tmpdir(), "perry_gap_instanceof_" + process.pid);
fs.writeFileSync(file, "x");
const stream = fs.createReadStream(file);
for (let i = 0; i < 400; i++) {
  const asError = stream instanceof Error;
  const asBlob = stream instanceof Blob;
  if (asError || asBlob) console.log("unexpected", i);
}
console.log("error", stream instanceof Error);
console.log("blob", stream instanceof Blob);
console.log("plain object", ({}) instanceof Error);
stream.on("close", () => fs.unlinkSync(file));
stream.destroy();
