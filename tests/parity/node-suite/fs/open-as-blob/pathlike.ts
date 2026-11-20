import * as fs from "node:fs";
import { Buffer } from "node:buffer";

const ROOT = "/tmp/perry_node_suite_fs_open_as_blob_pathlike";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/file.txt";
fs.writeFileSync(file, "pathlike");

console.log("string:", await (await fs.openAsBlob(file)).text());
console.log("buffer:", await (await fs.openAsBlob(Buffer.from(file))).text());
console.log("url:", await (await fs.openAsBlob(new URL("file://" + file))).text());

const link = ROOT + "/link.txt";
try {
  fs.symlinkSync(file, link);
  console.log("symlink:", await (await fs.openAsBlob(link)).text());
} catch (e: any) {
  console.log("symlink error:", e?.code ?? e?.name);
}
