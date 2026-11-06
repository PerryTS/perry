import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stats_shape";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT);
fs.writeFileSync(ROOT + "/file.txt", "x");
fs.symlinkSync(ROOT + "/file.txt", ROOT + "/link.txt");
const st = fs.statSync(ROOT + "/file.txt");

console.log("isFile typeof:", typeof st.isFile);
console.log("isFile value:", st.isFile());
console.log("isDirectory typeof:", typeof st.isDirectory);
console.log("isDirectory value:", st.isDirectory());
console.log("isSymbolicLink typeof:", typeof st.isSymbolicLink);
console.log("isSymbolicLink value:", st.isSymbolicLink());
console.log("isBlockDevice typeof:", typeof (st as any).isBlockDevice);
console.log("isBlockDevice value:", (st as any).isBlockDevice());
console.log("isCharacterDevice typeof:", typeof (st as any).isCharacterDevice);
console.log("isCharacterDevice value:", (st as any).isCharacterDevice());
console.log("isFIFO typeof:", typeof (st as any).isFIFO);
console.log("isFIFO value:", (st as any).isFIFO());
console.log("isSocket typeof:", typeof (st as any).isSocket);
console.log("isSocket value:", (st as any).isSocket());

const lst = fs.lstatSync(ROOT + "/link.txt");
console.log("lstat symlink:", lst.isSymbolicLink());

const big = fs.statSync(ROOT + "/file.txt", { bigint: true });
console.log("bigint isFIFO:", typeof (big as any).isFIFO, (big as any).isFIFO());

if (process.platform === "win32") {
  console.log("dev null character:", "skipped");
} else {
  const devNull = fs.statSync("/dev/null");
  console.log(
    "dev null character:",
    typeof (devNull as any).isCharacterDevice,
    (devNull as any).isCharacterDevice(),
  );
}
