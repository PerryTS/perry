import { clearScreenDown } from "node:readline";
import { Writable } from "node:stream";

let output = "";
const writable = new Writable({
  write(chunk, _encoding, callback) {
    output += chunk;
    callback();
  },
});
console.log(clearScreenDown(writable), JSON.stringify(output));
writable.destroy();
