// parity-node-argv: --experimental-sqlite
import { DatabaseSync } from "node:sqlite";

function describe(label: string, statement: any) {
  const columns = statement.columns();
  console.log(label, "count", columns.length);
  for (const column of columns) {
    console.log(
      label,
      Object.getPrototypeOf(column) === null ? "null-proto" : "other-proto",
      String(column.name),
      String(column.column),
      String(column.table),
      String(column.database),
      String(column.type),
    );
  }
}

const db = new DatabaseSync(":memory:");
db.exec(`
  CREATE TABLE left_table(id INTEGER PRIMARY KEY, name TEXT, payload BLOB);
  CREATE TABLE right_table(owner INTEGER, score REAL);
`);
describe(
  "join",
  db.prepare(`
    SELECT l.id AS item_id, l.name, r.score, l.id + 1 AS next_id
    FROM left_table AS l JOIN right_table AS r ON r.owner = l.id
  `),
);
describe("write", db.prepare("INSERT INTO left_table(name) VALUES (?)"));

const closedStatement = db.prepare("SELECT name FROM left_table");
db.close();
try {
  closedStatement.columns();
  console.log("closed columns: OK");
} catch (error: any) {
  console.log("closed columns: THROW", error.name, error.code || "no-code");
}
