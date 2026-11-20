import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_opendir_prototype_close";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
await fsp.writeFile(ROOT + "/a.txt", "A");

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

const shapeDir = await fsp.opendir(ROOT);
const proto = Object.getPrototypeOf(shapeDir);
console.log("promises dir own names:", ownNames(shapeDir));
console.log("promises dir proto names:", ownNames(proto));
console.log("promises dir own symbols:", ownSymbols(shapeDir));
console.log("promises dir proto symbols:", ownSymbols(proto));
console.log("promises dir own path:", String(hasOwn(shapeDir, "path")));
console.log("promises dir proto path:", String(hasOwn(proto, "path")));
console.log("promises dir path value:", String(shapeDir.path === ROOT));
for (const key of ["constructor", "path", "readSync", "closeSync", "read", "close", "entries"]) {
  console.log("promises dir desc:", descriptorLine(proto, key));
}
console.log(
  "promises dir symbol types:",
  typeof (shapeDir as any)[Symbol.asyncIterator],
  typeof (shapeDir as any)[Symbol.dispose],
  typeof (shapeDir as any)[Symbol.asyncDispose],
);
await shapeDir.close();

const promiseCloseDir = await fsp.opendir(ROOT);
console.log("promises dir close first:", String((await promiseCloseDir.close()) === undefined));
try {
  await promiseCloseDir.close();
  console.log("promises dir close second:", "ok");
} catch (err: any) {
  console.log("promises dir close second:", err.code);
}

const syncCloseDir = await fsp.opendir(ROOT);
console.log("promises dir closeSync first:", String(syncCloseDir.closeSync() === undefined));
try {
  syncCloseDir.closeSync();
  console.log("promises dir closeSync second:", "ok");
} catch (err: any) {
  console.log("promises dir closeSync second:", err.code);
}

try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
