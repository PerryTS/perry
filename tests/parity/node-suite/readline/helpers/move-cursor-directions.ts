import { moveCursor } from "node:readline";
import { Writable } from "node:stream";

for (const [dx, dy] of [[0, 0], [1, 0], [-1, 0], [0, 1], [0, -1], [1, -1]]) {
  let output = "";
  const writable = new Writable({
    write(chunk, _encoding, callback) {
      output += chunk;
      callback();
    },
  });
  console.log(dx, dy, moveCursor(writable, dx, dy), JSON.stringify(output));
  writable.destroy();
}
