(process as any).emitWarning = () => {};
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_glob_advanced";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT + "/a/b", { recursive: true });
fs.mkdirSync(ROOT + "/node_modules/pkg", { recursive: true });
fs.mkdirSync(ROOT + "/dist", { recursive: true });
fs.writeFileSync(ROOT + "/root.txt", "root");
fs.writeFileSync(ROOT + "/a/three.txt", "three");
fs.writeFileSync(ROOT + "/a/one.js", "one");
fs.writeFileSync(ROOT + "/a/two.ts", "two");
fs.writeFileSync(ROOT + "/a/b/two.txt", "nested");
fs.writeFileSync(ROOT + "/a/b/skip.md", "skip");
fs.writeFileSync(ROOT + "/node_modules/pkg/mod.js", "mod");
fs.writeFileSync(ROOT + "/dist/out.js", "out");

const list = (value: string[]) => value.slice().sort().join(",");

if (typeof fs.globSync === "function") {
  const arrayMatches = fs.globSync(["a/**/*.txt", "**/*.txt", "a/b/two.txt"], { cwd: ROOT });
  console.log("globSync array dedupe:", list(arrayMatches));

  const excluded = fs.globSync("**/*.js", {
    cwd: ROOT,
    exclude: ["node_modules/**", "dist/**"],
  });
  console.log("globSync exclude:", list(excluded));

  console.log("globSync brace:", list(fs.globSync("a/*.{js,ts}", { cwd: ROOT })));
  console.log("globSync class:", list(fs.globSync("a/[ot]*.t?", { cwd: ROOT })));
  console.log("globSync extglob:", list(fs.globSync("a/@(one|two).+(js|ts)", { cwd: ROOT })));

  const abs = fs.globSync(ROOT + "/a/*.{js,ts}", { cwd: ROOT })
    .map((p) => p.slice(ROOT.length + 1));
  console.log("globSync absolute pattern:", list(abs));

  const dirents = fs.globSync("a/*.js", { cwd: ROOT, withFileTypes: true }) as fs.Dirent[];
  console.log(
    "globSync dirent:",
    dirents.map((d) => `${d.name}:${d.isFile()}:${d.isDirectory()}:${typeof (d as any).parentPath}`).join(","),
  );
}

if (typeof fs.glob === "function") {
  await new Promise<void>((resolve) => {
    fs.glob("**/*.{js,ts}", {
      cwd: ROOT,
      exclude: ["dist/**", "node_modules/**"],
    }, (err, matches) => {
      console.log("glob callback advanced err:", String(err === null));
      console.log("glob callback advanced:", list(matches));
      resolve();
    });
  });

  await new Promise<void>((resolve) => {
    fs.glob("a/*.js", { cwd: ROOT, withFileTypes: true }, (err, matches) => {
      const dirents = matches as fs.Dirent[];
      console.log("glob callback dirent err:", String(err === null));
      console.log(
        "glob callback dirent:",
        dirents.map((d) => `${d.name}:${d.isFile()}:${d.isDirectory()}`).join(","),
      );
      resolve();
    });
  });
}
