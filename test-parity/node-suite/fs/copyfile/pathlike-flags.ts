import * as fs from "node:fs";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_copyfile_pathlike_flags";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const src = ROOT + "/src []{}.txt";
const dst = ROOT + "/dst []{}.txt";
fs.writeFileSync(src, "source");
fs.copyFileSync(Buffer.from(src), pathToFileURL(dst));
console.log("copyFileSync buffer url:", fs.readFileSync(dst, "utf8"));

fs.writeFileSync(src, "new");
try { fs.copyFileSync(src, dst, fs.constants.COPYFILE_EXCL); } catch (_e) {}
console.log("copyFileSync excl kept dest:", fs.readFileSync(dst, "utf8"));

const cbDst = ROOT + "/callback.txt";
fs.copyFile(Buffer.from(src), Buffer.from(cbDst), (err) => {
  console.log("copyFile callback pathlike err:", err === null);
  console.log("copyFile callback pathlike:", fs.readFileSync(cbDst, "utf8"));
});
