// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(label, "OK", value?.value ?? value);
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");
const defaultStatement = db.prepare("SELECT $value AS value");
probe("default exact", () => defaultStatement.get({ value: 1 }));
probe("default unknown", () => defaultStatement.get({ value: 2, extra: 3 }));

const permissive = db.prepare("SELECT $value AS value", {
  allowUnknownNamedParameters: true,
});
probe("prepare allow", () => permissive.get({ value: 4, extra: 5 }));
permissive.setAllowUnknownNamedParameters(false);
probe("toggle deny", () => permissive.get({ value: 6, extra: 7 }));
permissive.setAllowUnknownNamedParameters(true);
probe("toggle allow", () => permissive.get({ value: 8, extra: 9 }));

const permissiveDb = new DatabaseSync(":memory:", {
  allowUnknownNamedParameters: true,
});
probe("database allow", () =>
  permissiveDb.prepare("SELECT $value AS value").get({ value: 10, extra: 11 }),
);
db.close();
permissiveDb.close();
