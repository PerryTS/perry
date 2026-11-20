import { emitKeypressEvents } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const keys: string[] = [];
const listener = (text: string | undefined) => keys.push(text ?? "_");
input.on("keypress", listener);
emitKeypressEvents(input);
input.write("a");
input.off("keypress", listener);
input.write("b");
input.on("keypress", listener);
input.write("c");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(keys.join("|"));
input.destroy();
