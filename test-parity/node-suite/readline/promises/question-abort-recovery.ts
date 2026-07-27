import { createInterface } from "node:readline/promises";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const rl = createInterface({ input, output, terminal: false });
const controller = new AbortController();
const events: string[] = [];
if (typeof (rl as any).on === "function") {
  (rl as any).on("line", (line: string) => events.push(`line:${line}`));
  const pending = rl.question("q> ", { signal: controller.signal }).catch((
    error,
  ) => events.push(error.name));
  controller.abort();
  input.write("ordinary\n");
  await pending;
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log(events.join("|"));
} else {
  console.log("missing");
}
rl.close();
input.destroy();
output.destroy();
