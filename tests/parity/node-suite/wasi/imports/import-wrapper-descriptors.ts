import { WASI } from "node:wasi";

const W: any = WASI;
const wasi = new W({ version: "preview1" });
const namespace = "wasi_snapshot_preview1";

if (typeof wasi.getImportObject !== "function") {
  console.log("getImportObject: unavailable");
} else {
  const first: any = wasi.getImportObject();
  const descriptor = Object.getOwnPropertyDescriptor(first, namespace);
  if (descriptor === undefined) {
    throw new TypeError("missing WASI namespace descriptor");
  }

  console.log(
    "namespace flags:",
    descriptor.enumerable,
    descriptor.configurable,
    descriptor.writable,
  );
  console.log("namespace identity:", descriptor.value === wasi.wasiImport);
  delete first[namespace];
  console.log("deleted from wrapper:", Object.keys(first).length === 0);
  console.log(
    "fresh wrapper unaffected:",
    wasi.getImportObject()[namespace] === wasi.wasiImport,
  );
}
