import { emitKeypressEvents } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const keys: string[] = [];
emitKeypressEvents(input);
input.on(
  "keypress",
  (text, key) =>
    keys.push(
      `${text ?? "_"}:${key.name}:${key.ctrl}:${key.meta}:${key.shift}:${
        JSON.stringify(key.sequence)
      }`,
    ),
);
input.write("\u001b[A\u001b[B\u001b[C\u001b[D");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(keys.join("|"));
input.destroy();
