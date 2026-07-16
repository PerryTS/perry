import { WASI } from "node:wasi";

const W: any = WASI;
const wasi = new W({ version: "preview1" });
const namespace = "wasi_snapshot_preview1";
const original = wasi.wasiImport;
const replacement = { replacement: true };

wasi.wasiImport = replacement;
console.log("instance replaced:", wasi.wasiImport === replacement);
console.log(
  "wrapper reflects replacement:",
  wasi.getImportObject()[namespace] === replacement,
);
console.log("original unchanged:", original !== replacement);
