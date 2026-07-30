import { clearScreenDown } from "node:readline";
import { Writable } from "node:stream";

const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
try {
  clearScreenDown(output, "bad" as any);
  console.log("ok");
} catch (error: any) {
  console.log(error.name, error.code);
} finally {
  output.destroy();
}
