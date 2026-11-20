import wasiDefault, * as wasiNamespace from "node:wasi";
import { WASI } from "node:wasi";

console.log("namespace keys:", Object.keys(wasiNamespace).sort().join(","));
console.log("default keys:", Object.keys(wasiDefault).sort().join(","));
console.log("default distinct:", wasiDefault !== wasiNamespace);
console.log(
  "WASI identities:",
  wasiDefault.WASI === WASI,
  wasiNamespace.WASI === WASI,
);
console.log("constructor metadata:", typeof WASI, WASI.name, WASI.length);
