import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stream_pathlike_encoding";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/stream.txt";
fs.writeFileSync(p, "abcdef");

await new Promise<void>((resolve) => {
  const rs = fs.createReadStream(Buffer.from(p), { start: 1, end: 3 });
  rs.on("data", (chunk) => {
    console.log("default chunk isBuffer:", Buffer.isBuffer(chunk));
    console.log("default chunk text:", chunk.toString("utf8"));
  });
  rs.on("end", () => resolve());
});

await new Promise<void>((resolve) => {
  const rs = fs.createReadStream(Buffer.from(p), "utf8");
  rs.on("data", (chunk) => {
    console.log("string option chunk type:", typeof chunk);
    console.log("string option text:", chunk);
  });
  rs.on("end", () => resolve());
});

const out = Buffer.from(ROOT + "/out.txt");
await new Promise<void>((resolve) => {
  const ws = fs.createWriteStream(out, { flags: "w" });
  ws.on("finish", () => resolve());
  ws.end(Buffer.from("written"));
});
console.log("write stream buffer path:", fs.readFileSync(out, "utf8"));
