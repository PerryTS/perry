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
probe("anonymous", () =>
  db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(1, "two", null),
);
probe("numbered", () =>
  db.prepare("INSERT INTO data VALUES (?2, ?1, ?2)").run("one", "two"),
);
probe("numbered gap", () =>
  db.prepare("INSERT INTO data VALUES (?1, ?3, ?1)").run("a", "unused", "c"),
);
probe("too many", () =>
  db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(1, 2, 3, 4),
);
probe("unbound", () => db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(9));

for (const row of db
  .prepare("SELECT a, b, c, typeof(c) AS tc FROM data ORDER BY rowid")
  .all() as any[]) {
  console.log("row:", String(row.a), String(row.b), String(row.c), row.tc);
}
db.close();
