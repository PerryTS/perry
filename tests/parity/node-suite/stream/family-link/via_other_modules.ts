// The stream and events families reached through other native modules: fs
// read streams, readline over a Readable, events.on/once, and the cluster
// default export (an EventEmitter).
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as readline from "node:readline";
import { Readable } from "node:stream";
import cluster from "node:cluster";
import { EventEmitter, once } from "node:events";

console.log("cluster is EventEmitter:", cluster instanceof EventEmitter, typeof cluster.on);
const file = path.join(os.tmpdir(), `perry-family-link-${process.pid}.txt`);
fs.writeFileSync(file, "line one\nline two\n");
const rs = fs.createReadStream(file);
console.log("ReadStream is Readable:", rs instanceof Readable);
const rl = readline.createInterface({ input: rs });
const lines: string[] = [];
rl.on("line", (l: string) => lines.push(l));
rl.on("close", async () => {
  console.log("lines", lines.join("|"));
  fs.unlinkSync(file);
  const ee = new EventEmitter();
  setTimeout(() => ee.emit("ping", 42), 1);
  const [v] = await once(ee, "ping");
  console.log("once", v);
});
