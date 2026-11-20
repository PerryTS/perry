import { clearLine } from "node:readline";
import { Writable } from "node:stream";

for (const direction of [-1, 0, 1]) {
  let output = "";
  const writable = new Writable({
    write(chunk, _encoding, callback) {
      output += chunk;
      callback();
    },
  });
  console.log(
    direction,
    clearLine(writable, direction),
    JSON.stringify(output),
  );
  writable.destroy();
}
