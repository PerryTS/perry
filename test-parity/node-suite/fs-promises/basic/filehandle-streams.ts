(process as any).emitWarning = () => {};
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_streams";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT);
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "alpha");

const rh = await fsp.open(p, "r");
const rs = rh.createReadStream();
let data = "";
let ended = false;
await new Promise((resolve) => {
  rs.on("data", (chunk) => { data += chunk.toString("utf8"); });
  rs.on("end", () => { ended = true; resolve(undefined); });
});
await rh.close();
console.log("fh read stream data:", data);
console.log("fh read stream ended:", ended);

const wh = await fsp.open(ROOT + "/out.txt", "w+");
const ws = wh.createWriteStream();
let finished = false;
await new Promise((resolve) => {
  ws.on("finish", () => { finished = true; resolve(undefined); });
  ws.write("be");
  ws.end("ta");
});
await wh.close();
console.log("fh write stream finished:", finished);
console.log("fh write stream content:", await fsp.readFile(ROOT + "/out.txt", "utf8"));
