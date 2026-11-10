import { WASI } from "node:wasi";

const W: any = WASI;

const wasi = new W({ version: "preview1" });
const ownDescriptor = Object.getOwnPropertyDescriptor(wasi, "wasiImport");
const prototypeDescriptor = Object.getOwnPropertyDescriptor(
  WASI.prototype,
  "wasiImport",
);

console.log("instanceof:", wasi instanceof WASI);
console.log(
  "prototype identity:",
  Object.getPrototypeOf(wasi) === WASI.prototype,
);
console.log("own enumerable keys:", Object.keys(wasi).join(","));
console.log("own names:", Object.getOwnPropertyNames(wasi).join(","));
console.log("wasiImport own:", ownDescriptor !== undefined);
console.log(
  "wasiImport own flags:",
  ownDescriptor?.enumerable ?? "-",
  ownDescriptor?.configurable ?? "-",
  ownDescriptor?.writable ?? "-",
);
console.log(
  "wasiImport prototype accessor:",
  prototypeDescriptor !== undefined,
  typeof prototypeDescriptor?.get,
  typeof prototypeDescriptor?.set,
);
console.log("string tag:", Object.prototype.toString.call(wasi));
