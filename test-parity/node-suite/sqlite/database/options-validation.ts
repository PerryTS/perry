// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function describe(value: any) {
  if (value === undefined) return "undefined";
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

function construct(label: string, path: any, options?: any) {
  try {
    const db = new DatabaseSync(path, options);
    console.log(label, "OK", db.isOpen);
    if (db.isOpen) db.close();
  } catch (error: any) {
    console.log(label, "THROW", error?.name, error?.code || "no-code");
  }
}

for (const path of [undefined, null, 0, true, {}, [], "bad\0path"]) {
  construct(`path ${describe(path)}`, path);
}
construct("buffer memory", Buffer.from(":memory:"));

for (const options of [null, 0, true, "x"]) {
  construct(`options ${describe(options)}`, ":memory:", options);
}

for (const name of [
  "open",
  "readOnly",
  "enableForeignKeyConstraints",
  "enableDoubleQuotedStringLiterals",
  "readBigInts",
  "returnArrays",
  "allowBareNamedParameters",
  "allowUnknownNamedParameters",
] as const) {
  construct(`option ${name}`, ":memory:", { [name]: 1 });
}

for (const timeout of [-1, 0.5, NaN, Infinity, "1"]) {
  construct(`timeout ${String(timeout)}`, ":memory:", { timeout });
}
