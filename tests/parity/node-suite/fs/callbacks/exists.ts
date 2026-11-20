(process as any).emitWarning = () => {};
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_exists_callback";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "exists");

await new Promise<void>((resolve) => {
  fs.exists(p, (exists) => {
    console.log("exists callback true:", exists);
    resolve();
  });
});
await new Promise<void>((resolve) => {
  fs.exists(ROOT + "/missing.txt", (exists) => {
    console.log("exists callback false:", exists);
    resolve();
  });
});
