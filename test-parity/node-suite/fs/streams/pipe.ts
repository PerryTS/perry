import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stream_pipe";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const src = ROOT + "/src.txt";
const dst = ROOT + "/dst.txt";
fs.writeFileSync(src, "pipe-data");

await new Promise<void>((resolve) => {
  const rs = fs.createReadStream(src, { highWaterMark: 3 });
  const ws = fs.createWriteStream(dst, { highWaterMark: 4 });
  ws.on("finish", () => resolve());
  rs.pipe(ws);
});

console.log("pipe content:", fs.readFileSync(dst, "utf8"));
