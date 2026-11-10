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
input.write("\u0001\u007f\u001bb\u001b[1;5D");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(keys.join("|"));
input.destroy();
