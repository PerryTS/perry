import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_mkdtemp_disposable_sync";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const disposable = (fs as any).mkdtempDisposableSync(ROOT + "/sync-");
console.log(
  "mkdtempDisposableSync shape:",
  typeof disposable.path,
  disposable.path.startsWith(ROOT + "/sync-"),
  typeof disposable.remove,
  typeof disposable[Symbol.dispose],
  Object.keys(disposable).sort().join(","),
);
fs.mkdirSync(disposable.path + "/nested");
fs.writeFileSync(disposable.path + "/nested/file.txt", "sync");
console.log("mkdtempDisposableSync nested exists:", fs.existsSync(disposable.path + "/nested/file.txt"));
console.log("mkdtempDisposableSync remove ret:", disposable.remove() === undefined);
console.log("mkdtempDisposableSync removed:", !fs.existsSync(disposable.path));
console.log("mkdtempDisposableSync remove idempotent:", disposable.remove() === undefined);

const symbolDisposable = (fs as any).mkdtempDisposableSync(ROOT + "/symbol-");
fs.writeFileSync(symbolDisposable.path + "/file.txt", "symbol");
console.log("mkdtempDisposableSync symbol ret:", symbolDisposable[Symbol.dispose]() === undefined);
console.log("mkdtempDisposableSync symbol removed:", !fs.existsSync(symbolDisposable.path));
console.log("mkdtempDisposableSync symbol idempotent:", symbolDisposable[Symbol.dispose]() === undefined);

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
