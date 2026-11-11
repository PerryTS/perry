// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function createDatabase() {
  const db = new DatabaseSync(":memory:");
  db.exec(`
    CREATE TABLE tracked(id INTEGER PRIMARY KEY, value TEXT) STRICT;
    CREATE TABLE ignored(id INTEGER PRIMARY KEY, value TEXT) STRICT;
  `);
  return db;
}

const source = createDatabase();
const session = source.createSession({ table: "tracked" });
source.prepare("INSERT INTO tracked VALUES (?, ?)").run(1, "tracked");
source.prepare("INSERT INTO ignored VALUES (?, ?)").run(1, "ignored");
const changeset = session.changeset();

const target = createDatabase();
console.log("apply:", target.applyChangeset(changeset));
console.log(
  "tracked:",
  (target.prepare("SELECT value FROM tracked").get() as any).value,
);
console.log(
  "ignored:",
  target.prepare("SELECT value FROM ignored").all().length,
);

session.close();
source.close();
target.close();
