import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stream_backpressure";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const out = ROOT + "/out.txt";

const ws = fs.createWriteStream(out, { highWaterMark: 4 });
const events: string[] = [];
ws.on("drain", () => {
  events.push(`drain:${ws.writableNeedDrain}:${ws.writableLength}`);
});

const ret = ws.write(Buffer.from("abcd"));
console.log("write backpressure:", ret, ws.writableNeedDrain, ws.writableLength);
await new Promise((resolve) => setTimeout(resolve, 20));
console.log("write after drain:", events.join(","), ws.writableNeedDrain, ws.writableLength);

await new Promise<void>((resolve) => {
  ws.on("finish", () => resolve());
  ws.end("ef");
});
console.log("write backpressure content:", fs.readFileSync(out, "utf8"));
