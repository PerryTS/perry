import { WASI } from "node:wasi";

const W: any = WASI;
const preview1 = new W({ version: "preview1" });
const previewImport = preview1.wasiImport;
const requiredImports = [
  "args_get",
  "args_sizes_get",
  "clock_res_get",
  "clock_time_get",
  "environ_get",
  "environ_sizes_get",
  "fd_read",
  "fd_write",
  "proc_exit",
  "random_get",
];

console.log("instanceof:", preview1 instanceof W);
console.log("own keys:", Object.keys(preview1).join(","));
console.log("wasiImport own/object:", Object.hasOwn(preview1, "wasiImport"), typeof previewImport);
console.log("wasiImport self:", previewImport === preview1);
console.log("wasiImport key count:", Object.keys(previewImport).length);
console.log("required import functions:", requiredImports.every((key) => typeof previewImport[key] === "function"));
console.log("required import types:", requiredImports.map((key) => typeof previewImport[key]).join(","));

const previewObject1 = preview1.getImportObject();
const previewObject2 = preview1.getImportObject();
console.log("preview wrapper keys:", Object.keys(previewObject1).join(","));
console.log(
  "preview identities:",
  previewObject1.wasi_snapshot_preview1 === previewImport,
  previewObject2.wasi_snapshot_preview1 === previewImport,
  previewObject1 === previewObject2,
);

const unstable = new W({ version: "unstable" });
const unstableObject = unstable.getImportObject();
console.log("unstable wrapper keys:", Object.keys(unstableObject).join(","));
console.log("unstable identity:", unstableObject.wasi_unstable === unstable.wasiImport);
console.log("method identities:", preview1.getImportObject === W.prototype.getImportObject);
console.log("prototype methods:", ["getImportObject", "start", "initialize", "finalizeBindings"].map((name) => typeof W.prototype[name]).join(","));
