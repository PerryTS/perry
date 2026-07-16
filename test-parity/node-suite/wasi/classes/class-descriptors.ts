import { WASI } from "node:wasi";

const W: any = WASI;

function flags(object: object, key: PropertyKey) {
  const descriptor = Object.getOwnPropertyDescriptor(object, key)!;
  return [descriptor.enumerable, descriptor.configurable, descriptor.writable]
    .join("/");
}

console.log(
  "prototype names:",
  Object.getOwnPropertyNames(WASI.prototype).sort().join(","),
);
for (
  const key of [
    "constructor",
    "finalizeBindings",
    "getImportObject",
    "initialize",
    "start",
  ]
) {
  const value = (WASI.prototype as any)[key];
  console.log(
    key + ":",
    typeof value,
    value.name,
    value.length,
    flags(WASI.prototype, key),
  );
}
console.log("constructor link:", WASI.prototype.constructor === WASI);
