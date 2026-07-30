import { emitKeypressEvents } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const keys: string[] = [];
emitKeypressEvents(input);
input.on(
  "keypress",
  (_text, key) =>
    keys.push(`${key.name}:${key.ctrl}:${JSON.stringify(key.sequence)}`),
);
input.write("\u001b");
input.write("[1;5D");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(keys.join("|"));
input.destroy();
