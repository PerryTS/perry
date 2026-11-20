import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_mkdtemp_disposable";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const disposable = (fs as any).mkdtempDisposableSync(ROOT + "/sync-");
console.log("mkdtempDisposableSync keys:", JSON.stringify(Object.keys(disposable)));
console.log("mkdtempDisposableSync path prefix:", disposable.path.startsWith(ROOT + "/sync-"));
console.log("mkdtempDisposableSync exists:", fs.existsSync(disposable.path));
console.log("mkdtempDisposableSync remove type:", typeof disposable.remove, disposable.remove.length);
console.log("mkdtempDisposableSync dispose type:", typeof disposable[Symbol.dispose], disposable[Symbol.dispose].length);
fs.mkdirSync(disposable.path + "/nested");
fs.writeFileSync(disposable.path + "/nested/file.txt", "nested");
disposable.remove();
console.log("mkdtempDisposableSync removed:", fs.existsSync(disposable.path));
disposable.remove();
console.log("mkdtempDisposableSync idempotent:", fs.existsSync(disposable.path));

const chdirDisposable = (fs as any).mkdtempDisposableSync(ROOT + "/chdir-");
const cwd = process.cwd();
process.chdir("/");
chdirDisposable.remove();
process.chdir(cwd);
console.log("mkdtempDisposableSync chdir removed:", fs.existsSync(chdirDisposable.path));

try {
  (fs as any).mkdtempDisposableSync(ROOT + "/buffer-", { encoding: "buffer" });
  console.log("mkdtempDisposableSync buffer no-throw");
} catch (err: any) {
  console.log("mkdtempDisposableSync buffer code:", err && err.code);
  console.log("mkdtempDisposableSync buffer name:", err && err.name);
}
