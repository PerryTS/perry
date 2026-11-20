// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(label, "OK", value?.changes ?? value);
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE data(a, b, c)");
probe("duplicate bare", () =>
  db
    .prepare("INSERT INTO data VALUES ($value, $value, $value)")
    .run({ value: 1 }),
);
probe("duplicate prefixed", () =>
  db
    .prepare("INSERT INTO data VALUES ($value, $value, $value)")
    .run({ $value: 2 }),
);
probe("ambiguous bare", () =>
  db
    .prepare("INSERT INTO data VALUES ($value, @value, :value)")
    .run({ value: 3 }),
);
probe("ambiguous prefixed", () =>
  db
    .prepare("INSERT INTO data VALUES ($value, @value, :value)")
    .run({ $value: 3, "@value": 4, ":value": 5 }),
);
console.log(
  "rows:",
  (db.prepare("SELECT a, b, c FROM data ORDER BY rowid").all() as any[])
    .map((row) => `${row.a},${row.b},${row.c}`)
    .join("|"),
);
db.close();
