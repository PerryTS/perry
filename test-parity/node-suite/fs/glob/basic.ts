(process as any).emitWarning = () => {};
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_glob";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/a/b", { recursive: true });
fs.writeFileSync(ROOT + "/root.txt", "root");
fs.writeFileSync(ROOT + "/a/one.txt", "one");
fs.writeFileSync(ROOT + "/a/b/two.txt", "two");
fs.writeFileSync(ROOT + "/a/b/skip.md", "skip");

if (typeof fs.globSync === "function") {
  const matches = fs.globSync(ROOT + "/**/*.txt").sort();
  console.log("globSync count:", matches.length);
  console.log("globSync suffixes:", matches.map((p) => p.slice(ROOT.length + 1)).join(","));
}
if (typeof fs.glob === "function") {
  fs.glob(ROOT + "/**/*.txt", (err, matches) => {
    console.log("glob callback err:", err === null);
    console.log("glob callback count:", matches.sort().length);
  });
}
