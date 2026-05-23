(process as any).emitWarning = () => {};
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_glob_cwd_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/a/b", { recursive: true });
fs.mkdirSync(ROOT + "/c", { recursive: true });
fs.writeFileSync(ROOT + "/root.txt", "root");
fs.writeFileSync(ROOT + "/a/one.txt", "one");
fs.writeFileSync(ROOT + "/a/b/two.txt", "two");
fs.writeFileSync(ROOT + "/c/three.md", "three");

if (typeof fs.globSync === "function") {
  const cwdMatches = fs.globSync("**/*.txt", { cwd: ROOT }).sort();
  console.log("globSync cwd:", cwdMatches.join(","));
  const nestedMatches = fs.globSync("a/**/*.txt", { cwd: ROOT }).sort();
  console.log("globSync cwd nested:", nestedMatches.join(","));
}

if (typeof fs.glob === "function") {
  await new Promise<void>((resolve) => {
    fs.glob("**/*.txt", { cwd: ROOT }, (err, matches) => {
      console.log("glob callback cwd err:", err === null);
      console.log("glob callback cwd:", matches.sort().join(","));
      resolve();
    });
  });
}
