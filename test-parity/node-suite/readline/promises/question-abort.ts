import { createInterface } from "node:readline/promises";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const writes: string[] = [];
const output = new Writable({
  write(chunk, _encoding, callback) {
    writes.push(String(chunk));
    callback();
  },
});

const rl = createInterface({ input, output, terminal: false });
const controller = new AbortController();
const pending = rl.question("abort> ", { signal: controller.signal }).catch((err) => err);
controller.abort();
const result = await pending;
await new Promise<void>((resolve) => setImmediate(resolve));
rl.close();

console.log(
  "abort:",
  result.constructor.name,
  result.name,
  result.code,
  result.message,
);
console.log("writes:", JSON.stringify(writes.join("")));
