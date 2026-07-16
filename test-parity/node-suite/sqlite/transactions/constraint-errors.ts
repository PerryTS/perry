// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(label, "OK", value?.changes ?? value);
  } catch (error: any) {
    console.log(
      label,
      "THROW",
      error.name,
      error.code || "no-code",
      typeof error.errcode === "number" ? error.errcode : "no-errcode",
    );
  }
}

const db = new DatabaseSync(":memory:");
db.exec(`
  CREATE TABLE data(
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    score INTEGER CHECK(score >= 0)
  ) STRICT;
`);
probe("valid", () =>
  db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(1, "one", 1),
);
probe("primary", () =>
  db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(1, "two", 2),
);
probe("unique", () =>
  db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(2, "one", 2),
);
probe("not null", () =>
  db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(3, null, 3),
);
probe("check", () =>
  db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(4, "four", -1),
);
probe("strict type", () =>
  db.prepare("INSERT INTO data VALUES (?, ?, ?)").run(5, "five", "bad"),
);
console.log("survivors:", db.prepare("SELECT count(*) AS n FROM data").get().n);
db.close();
