// Direct fs calls must root their native registry without creating the namespace.
// Expected output: 1500
import fs from "node:fs";
import { devNull } from "node:os";
function run() {
  let done = 0;
  const next = () => {
    if (++done === 1500) return console.log(done);
    fs.createReadStream(devNull).on("end", () => next()).resume();
  };
  next();
}
run();
