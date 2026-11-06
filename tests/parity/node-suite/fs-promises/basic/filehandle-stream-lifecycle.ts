import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_filehandle_stream_lifecycle";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "abcdef");

const rh = await fsp.open(p, "r");
const readChunks: string[] = [];
await new Promise<void>((resolve) => {
  const rs = rh.createReadStream({ highWaterMark: 2 });
  rs.on("data", (chunk) => readChunks.push(chunk.toString("utf8")));
  rs.on("close", () => resolve());
});
console.log("fh lifecycle read chunks:", readChunks.join("|"));
console.log("fh lifecycle read fd after stream:", rh.fd);
await rh.close();

const rh2 = await fsp.open(p, "r");
await new Promise<void>((resolve) => {
  const rs = rh2.createReadStream({ autoClose: false });
  rs.on("end", () => {
    setTimeout(() => {
      console.log("fh lifecycle autoClose false fd alive:", rh2.fd >= 0);
      resolve();
    }, 5);
  });
  rs.resume();
});
await rh2.close();

const wh = await fsp.open(ROOT + "/out.txt", "w+");
await new Promise<void>((resolve) => {
  const ws = wh.createWriteStream({ highWaterMark: 3 });
  ws.on("close", () => resolve());
  ws.write("abc");
  ws.end("def");
});
console.log("fh lifecycle write fd after stream:", wh.fd);
await wh.close();
console.log("fh lifecycle write content:", await fsp.readFile(ROOT + "/out.txt", "utf8"));
