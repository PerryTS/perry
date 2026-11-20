import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_opendir_prototype_close";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
fs.writeFileSync(ROOT + "/a.txt", "A");

function ownNames(value: any) {
  return Object.getOwnPropertyNames(value).sort().join(",");
}

function ownSymbols(value: any) {
  return Object.getOwnPropertySymbols(value).map(String).sort().join(",");
}

function hasOwn(value: any, key: string) {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function descriptorLine(proto: any, key: string) {
  const desc = Object.getOwnPropertyDescriptor(proto, key) as any;
  if ("value" in desc) {
    return `${key}:data:${typeof desc.value}:${String(desc.writable)}:${String(desc.enumerable)}:${String(desc.configurable)}`;
  }
  return `${key}:accessor:${typeof desc.get}:${typeof desc.set}:${String(desc.enumerable)}:${String(desc.configurable)}`;
}

const shapeDir = fs.opendirSync(ROOT);
const proto = Object.getPrototypeOf(shapeDir);
console.log("dir own names:", ownNames(shapeDir));
console.log("dir proto names:", ownNames(proto));
console.log("dir own symbols:", ownSymbols(shapeDir));
console.log("dir proto symbols:", ownSymbols(proto));
console.log("dir own path:", String(hasOwn(shapeDir, "path")));
console.log("dir proto path:", String(hasOwn(proto, "path")));
console.log("dir path value:", String(shapeDir.path === ROOT));
for (const key of ["constructor", "path", "readSync", "closeSync", "read", "close", "entries"]) {
  console.log("dir desc:", descriptorLine(proto, key));
}
console.log(
  "dir symbol types:",
  typeof (shapeDir as any)[Symbol.asyncIterator],
  typeof (shapeDir as any)[Symbol.dispose],
  typeof (shapeDir as any)[Symbol.asyncDispose],
);
shapeDir.closeSync();

const syncCloseDir = fs.opendirSync(ROOT);
console.log("dir closeSync first:", String(syncCloseDir.closeSync() === undefined));
try {
  syncCloseDir.closeSync();
  console.log("dir closeSync second:", "ok");
} catch (err: any) {
  console.log("dir closeSync second:", err.code);
}

const callbackDir = await new Promise<fs.Dir>((resolve, reject) => {
  fs.opendir(ROOT, (err, dir) => {
    if (err) reject(err);
    else resolve(dir as fs.Dir);
  });
});
await new Promise<void>((resolve) => {
  callbackDir.close((err: any) => {
    console.log("dir close callback first:", String(err === null));
    resolve();
  });
});
await new Promise<void>((resolve) => {
  callbackDir.close((err: any) => {
    console.log("dir close callback second:", err.code);
    resolve();
  });
});

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
