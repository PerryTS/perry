import { WASI } from "node:wasi";

const W: any = WASI;

const wasi = new W({ version: "preview1" });
const descriptor = Object.getOwnPropertyDescriptor(wasi, "wasiImport")!;

console.log("instanceof:", wasi instanceof WASI);
console.log(
  "prototype identity:",
  Object.getPrototypeOf(wasi) === WASI.prototype,
);
console.log("own enumerable keys:", Object.keys(wasi).join(","));
console.log("own names:", Object.getOwnPropertyNames(wasi).join(","));
console.log(
  "wasiImport flags:",
  descriptor.enumerable,
  descriptor.configurable,
  descriptor.writable,
);
console.log("string tag:", Object.prototype.toString.call(wasi));
