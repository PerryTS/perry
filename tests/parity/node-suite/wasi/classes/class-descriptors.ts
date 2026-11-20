import { WASI } from "node:wasi";

function flags(object: object, key: PropertyKey) {
  const descriptor = Object.getOwnPropertyDescriptor(object, key);
  if (descriptor === undefined) return "missing";
  return [
    descriptor.enumerable,
    descriptor.configurable,
    descriptor.writable,
  ].join("/");
}

console.log(
  "constructor names:",
  Object.getOwnPropertyNames(WASI).sort().join(","),
);
for (const key of ["length", "name", "prototype"]) {
  console.log("constructor " + key + ":", flags(WASI, key));
}
console.log(
  "constructor parent:",
  Object.getPrototypeOf(WASI) === Function.prototype,
);
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
    typeof value === "function" ? value.name : "-",
    typeof value === "function" ? value.length : "-",
    flags(WASI.prototype, key),
  );
}
console.log("constructor link:", WASI.prototype.constructor === WASI);
