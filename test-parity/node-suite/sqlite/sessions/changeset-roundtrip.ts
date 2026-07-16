// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function createDatabase() {
  const db = new DatabaseSync(":memory:");
  db.exec("CREATE TABLE data(id INTEGER PRIMARY KEY, value TEXT) STRICT");
  return db;
}

function probe(label: string, fn: () => unknown) {
  try {
    const value: any = fn();
    console.log(
      label,
      "OK",
      value instanceof Uint8Array ? `bytes:${value.length}` : String(value),
    );
  } catch (error: any) {
    console.log(label, "THROW", error.name, error.code || "no-code");
  }
}

const source = createDatabase();
const session = source.createSession();
source.prepare("INSERT INTO data VALUES (?, ?)").run(1, "one");
source.prepare("INSERT INTO data VALUES (?, ?)").run(2, "two");
const changeset = session.changeset();
const patchset = session.patchset();
console.log(
  "sets:",
  changeset instanceof Uint8Array,
  changeset.length > 0,
  patchset instanceof Uint8Array,
  patchset.length > 0,
);

const target = createDatabase();
console.log("apply:", target.applyChangeset(changeset));
console.log(
  "rows:",
  (
    target
      .prepare("SELECT id || ':' || value AS row FROM data ORDER BY id")
      .all() as any[]
  )
    .map((row) => row.row)
    .join(","),
);

session.close();
probe("changeset closed session", () => session.changeset());
source.close();
target.close();
probe("create closed db", () => source.createSession());
probe("apply closed db", () => target.applyChangeset(changeset));
