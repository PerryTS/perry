import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const events: string[] = [];
const rl = createInterface({ input, terminal: false });
rl.on("line", (line) => events.push(`line:${line}`));
rl.on("close", () => events.push("close"));
input.end("one\ntwo");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(events.join("|"));
rl.close();
input.destroy();
