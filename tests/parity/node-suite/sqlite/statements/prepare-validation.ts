// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(label, "OK", typeof value);
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");
probe("valid", () => db.prepare("SELECT 1"));
probe("syntax", () => db.prepare("SELEC 1"));
probe("multiple", () => db.prepare("SELECT 1; SELECT 2"));
probe("empty", () => db.prepare(""));
probe("nul", () => db.prepare("SELECT\0 1"));
for (const value of [undefined, null, 1, true, {}, []]) {
  probe(
    `value ${value === null ? "null" : Array.isArray(value) ? "array" : typeof value}`,
    () => db.prepare(value as any),
  );
}
for (const options of [null, 1, true, "options"]) {
  probe(`options ${options === null ? "null" : typeof options}`, () =>
    db.prepare("SELECT 1", options as any),
  );
}
db.close();
