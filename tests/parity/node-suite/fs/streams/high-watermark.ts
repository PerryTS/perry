import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stream_high_watermark";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/chunks.txt";
fs.writeFileSync(p, "abcdefghij");

await new Promise<void>((resolve) => {
  const chunks: string[] = [];
  const rs = fs.createReadStream(p, { highWaterMark: 3 });
  rs.on("data", (chunk) => {
    chunks.push(`${Buffer.isBuffer(chunk)}:${chunk.toString("utf8")}`);
  });
  rs.on("end", () => {
    console.log("read hwm buffer chunks:", chunks.join("|"));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  const chunks: string[] = [];
  const rs = fs.createReadStream(p, { encoding: "utf8", highWaterMark: 4, start: 2, end: 8 });
  rs.on("data", (chunk) => {
    chunks.push(`${typeof chunk}:${chunk}`);
  });
  rs.on("end", () => {
    console.log("read hwm encoding chunks:", chunks.join("|"));
    resolve();
  });
});
