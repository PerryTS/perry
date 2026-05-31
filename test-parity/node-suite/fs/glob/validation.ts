(process as any).emitWarning = () => {};
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_glob_validation";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/x.txt", "x");

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "ok";
  } catch (err) {
    return (err as any).code || (err as Error).name;
  }
}

if (typeof fs.globSync === "function") {
  console.log("globSync invalid pattern:", codeOf(() => fs.globSync(42 as any)));
  console.log("globSync invalid options:", codeOf(() => fs.globSync("*", null as any)));
  console.log("globSync invalid cwd:", codeOf(() => fs.globSync("*", { cwd: 42 as any })));
  console.log("globSync invalid exclude:", codeOf(() => fs.globSync("*", { exclude: "x.txt" as any })));
  console.log("globSync invalid exclude item:", codeOf(() => fs.globSync("*", { exclude: ["x.txt", 42 as any] })));
}

if (typeof fs.glob === "function") {
  console.log("glob callback missing:", codeOf(() => fs.glob("*", null as any)));
  await new Promise<void>((resolve) => {
    fs.glob("*", { cwd: 42 as any }, (err) => {
      console.log("glob callback cwd err:", (err as any)?.code || "none");
      resolve();
    });
  });
}
