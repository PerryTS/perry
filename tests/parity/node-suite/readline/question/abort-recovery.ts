import { createInterface } from "node:readline";
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
  rl.on("line", (line) => events.push(`line:${line}`));
  rl.question(
    "q> ",
    { signal: controller.signal },
    () => events.push("answer"),
  );
  controller.abort();
  input.write("ordinary\n");
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log(events.join("|"), rl.getPrompt());
} else {
  console.log("missing");
}
rl.close();
input.destroy();
output.destroy();
