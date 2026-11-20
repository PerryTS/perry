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
const named = db.prepare("INSERT INTO data VALUES ($a, :b, @c)");
probe("prefixed", () => named.run({ $a: 1, ":b": 2, "@c": 3 }));
probe("bare", () => named.run({ a: 4, b: 5, c: 6 }));
probe("mixed", () =>
  db.prepare("INSERT INTO data VALUES ($a, ?, ?)").run({ a: 7 }, 8, 9),
);
probe("missing", () => named.run({ a: 10, b: 11 }));
probe("primitive map", () => named.run(1 as any));

const strict = db.prepare("SELECT $value AS value", {
  allowBareNamedParameters: false,
});
probe("strict prefixed", () => (strict.get({ $value: "yes" }) as any).value);
probe("strict bare", () => strict.get({ value: "no" }));

for (const row of db
  .prepare("SELECT a, b, c FROM data ORDER BY rowid")
  .all() as any[]) {
  console.log("row:", row.a, row.b, row.c);
}
db.close();
