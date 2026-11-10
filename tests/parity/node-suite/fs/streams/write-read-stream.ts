import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stream";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/stream.txt";
await new Promise<void>((resolve) => {
  const ws = fs.createWriteStream(p);
  ws.on("finish", () => resolve());
  ws.write("stream");
  ws.end(" data");
});
console.log("write stream content:", fs.readFileSync(p, "utf8"));
await new Promise<void>((resolve) => {
  let text = "";
  const rs = fs.createReadStream(p, { encoding: "utf8" });
  rs.on("data", (chunk) => { text += chunk; });
  rs.on("end", () => { console.log("read stream content:", text); resolve(); });
});
