import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stream_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/stream.txt";
fs.writeFileSync(p, "0123456789");

await new Promise<void>((resolve) => {
  let text = "";
  const rs = fs.createReadStream(p, { encoding: "utf8", start: 2, end: 5 });
  rs.on("data", (chunk) => { text += chunk; });
  rs.on("end", () => {
    console.log("read stream start end:", text);
    resolve();
  });
});

const appendPath = ROOT + "/append.txt";
fs.writeFileSync(appendPath, "A");
await new Promise<void>((resolve) => {
  const ws = fs.createWriteStream(appendPath, { flags: "a" });
  ws.on("finish", () => resolve());
  ws.write("B");
  ws.end("C");
});
console.log("write stream append flags:", fs.readFileSync(appendPath, "utf8"));

const wxPath = ROOT + "/wx.txt";
await new Promise<void>((resolve) => {
  const ws = fs.createWriteStream(wxPath, { flags: "wx" });
  ws.on("finish", () => resolve());
  ws.end("created");
});
console.log("write stream wx create:", fs.readFileSync(wxPath, "utf8"));
