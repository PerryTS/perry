import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_mkdtemp_disposable";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const disposable = await (fsp as any).mkdtempDisposable(ROOT + "/prom-");
console.log("promises mkdtempDisposable keys:", JSON.stringify(Object.keys(disposable)));
console.log("promises mkdtempDisposable path prefix:", disposable.path.startsWith(ROOT + "/prom-"));
console.log("promises mkdtempDisposable exists:", fs.existsSync(disposable.path));
console.log("promises mkdtempDisposable remove type:", typeof disposable.remove, disposable.remove.length);
console.log("promises mkdtempDisposable asyncDispose type:", typeof disposable[Symbol.asyncDispose], disposable[Symbol.asyncDispose].length);
fs.mkdirSync(disposable.path + "/nested");
fs.writeFileSync(disposable.path + "/nested/file.txt", "nested");
const removeResult = disposable.remove();
console.log("promises mkdtempDisposable remove promise:", typeof removeResult.then === "function");
await removeResult;
console.log("promises mkdtempDisposable removed:", fs.existsSync(disposable.path));
await disposable.remove();
console.log("promises mkdtempDisposable idempotent:", fs.existsSync(disposable.path));

const chdirDisposable = await (fsp as any).mkdtempDisposable(ROOT + "/chdir-");
const cwd = process.cwd();
process.chdir("/");
await chdirDisposable.remove();
process.chdir(cwd);
console.log("promises mkdtempDisposable chdir removed:", fs.existsSync(chdirDisposable.path));

try {
  await (fsp as any).mkdtempDisposable(ROOT + "/buffer-", { encoding: "buffer" });
  console.log("promises mkdtempDisposable buffer resolved");
} catch (err: any) {
  console.log("promises mkdtempDisposable buffer code:", err && err.code);
  console.log("promises mkdtempDisposable buffer name:", err && err.name);
}
