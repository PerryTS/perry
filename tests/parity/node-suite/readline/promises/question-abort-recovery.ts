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
const onLine = (line: string) => events.push(`line:${line}`);
if (typeof (rl as any).on === "function") (rl as any).on("line", onLine);
const pending = rl.question("q> ", { signal: controller.signal }).catch((
  error,
) => events.push(error.name));
controller.abort();
input.write("ordinary\n");
await pending;
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(events.join("|"));
if (typeof (rl as any).off === "function") (rl as any).off("line", onLine);
rl.close();
input.destroy();
output.destroy();
