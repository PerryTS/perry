import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_filehandle_stream_options";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "0123456789");

const rh = await fsp.open(p, "r");
await new Promise<void>((resolve) => {
  const rs = rh.createReadStream({ start: 2, end: 5 });
  rs.on("data", (chunk) => {
    console.log("fh stream default isBuffer:", Buffer.isBuffer(chunk));
    console.log("fh stream range:", chunk.toString("utf8"));
  });
  rs.on("end", () => resolve());
});
await rh.close();

const rh2 = await fsp.open(p, "r");
await new Promise<void>((resolve) => {
  const rs = rh2.createReadStream({ encoding: "utf8", start: 6, end: 8 });
  rs.on("data", (chunk) => {
    console.log("fh stream encoding type:", typeof chunk);
    console.log("fh stream encoding range:", chunk);
  });
  rs.on("end", () => resolve());
});
await rh2.close();

const wh = await fsp.open(ROOT + "/append.txt", "w+");
await wh.writeFile("A");
await new Promise<void>((resolve) => {
  const ws = wh.createWriteStream({ flags: "a" });
  ws.on("finish", () => resolve());
  ws.end("B");
});
await wh.close();
console.log("fh write stream append:", await fsp.readFile(ROOT + "/append.txt", "utf8"));
